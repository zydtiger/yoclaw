# Yoclaw Architecture: Coroutine Analysis

## Overview

This document details all coroutines spawned using `tokio::spawn`, their main actors, owned resources, and inter-corsosnt communication patterns.

**Architecture Version:** 2.1 (Agent in Main Thread - 2 coroutines, deadlock-safe)

---

## All `tokio::spawn` Locations

| #   | Location                    | Purpose                                       |
| --- | --------------------------- | --------------------------------------------- |
| 1   | `src/main.rs` (lines 35-72) | **Unified Telegram** coroutine (poll + send)  |
| 2   | `src/main.rs` (lines 90-95) | **TaskProcessor** coroutine (avoids deadlock) |

---

## 1. Unified Telegram Coroutine

**Location:** `src/main.rs` (lines 35-72)

### Main Actor

- **Telegram Handler** - A single coroutine that handles both polling incoming messages AND sending outgoing messages using `tokio::select!`

### Resources Owned

| Resource          | Type                     | Ownership                                |
| ----------------- | ------------------------ | ---------------------------------------- |
| `TelegramChannel` | `Arc<TelegramChannel>`   | **Shared** (channel_poll + channel_send) |
| `rx`              | `mpsc::Receiver<String>` | **Exclusive**                            |
| `chat_id`         | `&str` ("7235677031")    | **Exclusive** (hardcoded)                |
| `TaskManager`     | `Arc<TaskManager>`       | **Shared** (with Agent for tool calls)   |
| `shutdown_signal` | `Arc<Notify>`            | **Shared** (with TaskProcessor)          |

### Communication Patterns

| Direction    | Channel Type                                     | Purpose                                  |
| ------------ | ------------------------------------------------ | ---------------------------------------- |
| **Incoming** | Telegram API (HTTP polling)                      | Polls `/getUpdates` every 1 second       |
| **Incoming** | `mpsc::Receiver<String>` (rx)                    | Receives AI response messages from Agent |
| **Outgoing** | Telegram API (via `channel_send.send_message()`) | Sends messages to hardcoded chat_id      |

### Loop Structure

```rust
loop {
    tokio::select! {
        // Branch 1: Send outgoing messages
        Some(msg) = rx.recv() => {
            channel_send.send_message(chat_id, &msg).await;
        }

        // Branch 2: Poll incoming messages
        _ = sleep(Duration::from_secs(1)) => {
            let messages = channel.receive_messages().await;
            for msg in messages {
                task_manager.schedule_task(msg.text).await;
            }
        }
    }
}
```

**Lifecycle:** Runs indefinitely until the process is terminated

---

## 2. TaskProcessor Coroutine

**Location:** `src/main.rs` (lines 90-95)

### Main Actor

- **Task Queue Manager** - Manages the task queue, deadline scheduling, and persistence

### Resources Owned

| Resource          | Type                          | Ownership                                                    |
| ----------------- | ----------------------------- | ------------------------------------------------------------ |
| `TaskProcessor`   | `TaskProcessor` (owned)       | **Exclusive** - owns `BinaryHeap<Task>` and `mpsc::Receiver` |
| `task_rx`         | `mpsc::Receiver<TaskCommand>` | **Exclusive** (receives schedule/cancel/list commands)       |
| `pending_tasks`   | `BinaryHeap<Task>`            | **Exclusive** (priority queue ordered by deadline)           |
| `shutdown_signal` | `Arc<Notify>`                 | **Shared** (listens for shutdown from Telegram coroutine)    |

**Note:** The TaskProcessor runs in a **separate coroutine** to allow the Agent (in main thread) to use tools like `schedule_task` without deadlock.

### Communication Patterns

| Direction    | Channel Type                    | Purpose                                  |
| ------------ | ------------------------------- | ---------------------------------------- |
| **Incoming** | `mpsc::Receiver<TaskCommand>`   | Receives schedule/cancel/list commands   |
| **Outgoing** | `mpsc::Sender<Task>` (agent_tx) | Sends ready tasks to Agent (main thread) |
| **Internal** | `BinaryHeap` operations         | Manages task queue by deadline priority  |

### Loop Structure

```rust
loop {
    tokio::select! {
        // Branch 1: Task command arrives
        Some(msg) = self.task_rx.recv() => {
            match msg {
                TaskCommand::Schedule(task) => self.pending_tasks.push(task),
                TaskCommand::Cancel(task_id, reply_tx) => { /* remove task */ },
                TaskCommand::ListTasks(reply_tx) => { /* return tasks */ },
            }
        }

        // Branch 2: Timer fires (deadline reached)
        _ = sleep(sleep_duration) => {
            while let Some(task) = self.pending_tasks.peek() {
                if task.is_ready() {
                    let task = self.pending_tasks.pop().unwrap();
                    agent_tx.send(task).await;
                }
            }
        }

        // Branch 3: Shutdown signal
        _ = shutdown_signal.notified() => {
            self.save_tasks().await;
            break;
        }
    }
}
```

**Lifecycle:** Runs until shutdown signal received or channel closed

---

## Critical Design Decision: Why TaskProcessor Must Run in a Separate Coroutine

### The Deadlock Problem

If the TaskProcessor were on the **same thread** as the Agent (main thread), a **deadlock** would occur when the Agent uses tools that call back to TaskManager:

```
1. TaskProcessor sends task → Agent starts executing (on main thread)
2. Agent uses schedule_task tool → sends to TaskManager → TaskProcessor channel
3. TaskProcessor is BLOCKED waiting for Agent to finish
4. Agent is BLOCKED waiting for TaskProcessor to schedule new task
5. DEADLOCK!
```

### The Solution

By running the **TaskProcessor in a separate coroutine** (`tokio::spawn`):

- Agent (main thread) and TaskProcessor can proceed **concurrently**
- When Agent uses tools like `schedule_task`, `cancel_task`, or `list_tasks`, it can send messages to TaskProcessor without blocking
- TaskProcessor can process these messages while Agent continues executing

This allows the Agent to stay in the main thread (keeping `!Send` state local) while avoiding deadlocks.

---

## Communication Architecture (Mermaid Diagram)

```mermaid
sequenceDiagram
    autonumber
    participant P1 as Main Thread (Agent)
    participant P2 as TaskProcessor
    participant P3 as Channel Handler
    participant External as External Systems

    rect rgb(31, 90, 88)
        Note over P1, External: User task flow
        External->>P3: User message
        P3->>P2: Schedule immediate task (via TaskManager)
        P2->>P1: Dispatch task (via agent_tx)
        P1->>P1: Execute task (Agent.send_message)
        P1->>P3: Send response (via channel_tx)
        P3->>External: Forward to Telegram
    end

    rect rgb(41, 101, 50)
        Note over P1, External: Scheduled task flow
        P2->>P2: Deadline reached
        P2->>P1: Dispatch task (via agent_tx)
        P1->>P1: Execute task
        P1->>P3: Send response (via channel_tx)
        P3->>External: Forward to Telegram
    end

    rect rgb(90, 31, 31)
        Note over P1, External: Graceful shutdown flow
        External->>P3: SIGTERM (Ctrl+C)
        P3->>P2: Notify shutdown signal (Arc<Notify>)
        P2->>P2: Save pending tasks to tasks.json
        Note over P1, External: Application exits cleanly
    end
```

---

## Communication Channels Summary

| Channel               | Type                | Size | Sender             | Receiver           | Purpose                        |
| --------------------- | ------------------- | ---- | ------------------ | ------------------ | ------------------------------ |
| **Outgoing Messages** | `mpsc::String`      | 16   | Agent (main)       | Telegram Coroutine | Send AI responses to Telegram  |
| **Task Execution**    | `mpsc::Task`        | 32   | TaskProcessor      | Agent (main)       | Queue tasks for AI processing  |
| **Task Command**      | `mpsc::TaskCommand` | 100  | TaskManager (Arc)  | TaskProcessor      | Schedule/cancel/list tasks     |
| **Task Control**      | `oneshot::Result`   | 1    | TaskProcessor      | TaskManager        | Async response for cancel/list |
| **Shutdown Signal**   | `Arc<Notify>`       | N/A  | Telegram Coroutine | TaskProcessor      | Graceful shutdown coordination |

---

## Resource Ownership Matrix

| Resource                                | Owner               | Shared With                        | Access Pattern                                              |
| --------------------------------------- | ------------------- | ---------------------------------- | ----------------------------------------------------------- |
| **TelegramChannel**                     | Telegram Coroutine  | None (Arc clones within coroutine) | `Arc<TelegramChannel>` - concurrent read-only access        |
| **Agent (messages, tools)**             | Main Thread (Agent) | None                               | Exclusive ownership - single owner prevents race conditions |
| **TaskManager**                         | Main (Arc)          | Telegram Coroutine, Agent          | `Arc<TaskManager>` - concurrent access via channel          |
| **mpsc::Sender<String> (channel_tx)**   | Main Thread (Agent) | None                               | Exclusive ownership                                         |
| **mpsc::Receiver<String> (channel_rx)** | Telegram Coroutine  | None                               | Exclusive ownership                                         |
| **mpsc::Receiver<Task> (agent_rx)**     | Main Thread (Agent) | None                               | Exclusive ownership                                         |
| **BinaryHeap<Task>**                    | TaskProcessor       | None                               | Exclusive ownership - task queue                            |
| **shutdown_signal (Arc<Notify>)**       | Telegram Coroutine  | TaskProcessor                      | `Arc<Notify>` - Telegram triggers, TaskProcessor listens    |

---

## Key Design Patterns Used

### 1. **Exclusive Ownership for Mutable State**

- **Agent** is owned by the main thread, preventing concurrent access to `messages: Vec<Message>` and `tools: Vec<Tool>`
- This ensures conversation history integrity across all task executions

### 2. **Unified Coroutine via tokio::select!**

- Telegram polling and sending are merged into a **single coroutine**
- `tokio::select!` allows concurrent handling of both incoming and outgoing messages
- Eliminates the need for an intermediate `mpsc::String` channel between sender and poller

### 3. **TaskProcessor in Separate Coroutine (Deadlock Prevention)**

- TaskProcessor runs in a **separate coroutine** from the Agent (main thread)
- When Agent uses tools (schedule_task, cancel_task, list_tasks), it can call back to TaskManager without causing deadlock
- This is a critical design decision for system correctness

### 4. **Priority Queue with BinaryHeap**

- TaskProcessor uses `BinaryHeap<Task>` to prioritize tasks by deadline
- Tie-breaking by task ID ensures deterministic ordering

### 5. **Oneshot for Request-Response**

- Used for synchronous operations where the caller needs an immediate response:
  - `TaskManager::cancel_task()` → returns `Result<(), CancelError>` via oneshot
  - `TaskManager::list_tasks()` → returns `Vec<Task>` via oneshot

---

## Critical Observations

1. **No Race Conditions on Agent State**: The Agent is **owned by the main thread**, ensuring `messages: Vec<Message>` is never accessed concurrently.

2. **Deadlock-Free Design**: The separate TaskProcessor coroutine prevents deadlocks when Agent tools call back to TaskManager. This is a critical design decision.

3. **Simplified Telegram Handling**: Merging polling and sending into one coroutine reduces complexity and eliminates one hop in the message chain.

4. **Task Ordering**: Tasks are processed by deadline priority using `BinaryHeap`, ensuring time-sensitive tasks are handled first.

5. **Buffer Sizes**:
   - Outgoing messages: 16 (small, for immediate responses)
   - Task execution: 32 (medium, for backpressure)
   - Task commands: 100 (larger, for management operations)

6. **Error Handling**:
   - Telegram coroutine logs errors but continues polling
   - Agent panics on channel errors (expected behavior - indicates system shutdown)

---

## Graceful Shutdown

### Shutdown Signal Flow

The application implements graceful shutdown using `Arc<Notify>` to coordinate between coroutines:

```
1. User sends SIGTERM (Ctrl+C) → tokio::signal::ctrl_c() triggers in Telegram Coroutine
2. Telegram Coroutine calls shutdown_signal_clone.notify_waiters()
3. TaskProcessor receives shutdown signal via shutdown_signal.notified()
4. TaskProcessor saves pending tasks to tasks.json
5. Agent loop exits when channel closes
6. Application exits cleanly
```

### Implementation Details

**In `src/main.rs`:**

- Creates `shutdown_signal = Arc::new(Notify::new())`
- Telegram coroutine listens for `tokio::signal::ctrl_c()` (lines 35-44)
- On signal: logs message and calls `shutdown_clone.notify_waiters()`
- Passes `shutdown_signal.clone()` to `task_processor.run()`

**In `TaskProcessor::run()`:**

- Uses `shutdown_signal.notified()` in `tokio::select!`
- On shutdown: saves pending tasks to `tasks.json` before exiting

### Shutdown Triggers

| Trigger          | Source             | Handler                      | Action               |
| ---------------- | ------------------ | ---------------------------- | -------------------- |
| SIGTERM / Ctrl+C | Telegram Coroutine | `tokio::signal::ctrl_c()`    | Notify TaskProcessor |
| Channel closed   | TaskProcessor      | `tokio::select!` else branch | Save tasks and exit  |

---

## Architecture Evolution

### Version 1.0 (Original - 3 Coroutines)

```
Telegram Poller → TaskManager → TaskProcessor → Agent Worker → Telegram Sender → Telegram API
```

**Issues:**

- 3-hop response path (Agent Worker → Telegram Sender → Telegram API)
- Unnecessary `mpsc::String` channel between TaskProcessor and Telegram Sender
- Agent Worker coroutine added complexity without benefit

### Version 2.0 (Simplified - Agent in Worker Coroutine)

```
Telegram Coroutine (poll + send) ←→ TaskProcessor → Agent Worker → Telegram Coroutine
```

**Issues:**

- Agent in separate coroutine, TaskProcessor in main thread
- Deadlock risk when Agent uses tools that call back to TaskManager

### Version 2.1 (Agent in Main Thread - Deadlock-Safe)

```
Telegram Coroutine (poll + send) ←→ Agent (main) ←→ TaskProcessor (spawned)
```

**Benefits:**

- Agent in main thread keeps `!Send` state local
- TaskProcessor in separate coroutine allows concurrent tool calls
- Direct communication: Agent → Telegram coroutine (via channel_tx)
- Simpler mental model
- **Crucially: No deadlocks when Agent uses tools**
