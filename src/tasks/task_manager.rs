use chrono::Duration;
use chrono::Utc;
use std::collections::BinaryHeap;
use tokio::sync::{mpsc, oneshot};
use tokio::time::sleep;

use crate::tasks::{CancelError, Task, TaskCommand, TaskId};

/// TaskManager provides the interface for scheduling and cancelling tasks.
/// It runs in a separate tokio::spawn and sends tasks to the main TaskProcessor.
#[derive(Debug)]
pub struct TaskManager {
    tx: mpsc::Sender<TaskCommand>,
}

impl TaskManager {
    pub fn new(tx: mpsc::Sender<TaskCommand>) -> Self {
        Self { tx }
    }

    /// Schedule a task to run immediately
    pub async fn schedule_task(
        &self,
        payload: String,
    ) -> Result<TaskId, mpsc::error::SendError<TaskCommand>> {
        let task = Task::new(payload);
        let task_id = task.id;
        self.tx.send(TaskCommand::Schedule(task)).await?;
        Ok(task_id)
    }

    /// Schedule a task to run after a delay
    pub async fn schedule_task_in(
        &self,
        payload: String,
        delay: Duration,
    ) -> Result<TaskId, mpsc::error::SendError<TaskCommand>> {
        let task = Task::scheduled(payload, delay);
        let task_id = task.id;
        self.tx.send(TaskCommand::Schedule(task)).await?;
        Ok(task_id)
    }

    /// Cancel a pending task by its ID
    pub async fn cancel_task(&self, task_id: TaskId) -> Result<(), CancelError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .tx
            .send(TaskCommand::Cancel(task_id, reply_tx))
            .await
            .is_err()
        {
            panic!(
                "TaskProcessor channel is closed. Cannot cancel task {}",
                task_id
            );
        }
        match reply_rx.await {
            Ok(result) => result,
            Err(_) => Err(CancelError::NotFound),
        }
    }

    /// List all current pending tasks, their ids, payloads, and deadlines
    pub async fn list_tasks(&self) -> Vec<Task> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .tx
            .send(TaskCommand::ListTasks(reply_tx))
            .await
            .is_err()
        {
            return Vec::new(); // channel closed
        }
        reply_rx.await.unwrap_or_default()
    }
}

/// TaskProcessor handles the actual task queue and processing in the main loop.
/// Uses a priority queue (BinaryHeap) to manage tasks by deadline.
pub struct TaskProcessor {
    rx: mpsc::Receiver<TaskCommand>,
}

impl TaskProcessor {
    pub fn new(rx: mpsc::Receiver<TaskCommand>) -> Self {
        Self { rx }
    }

    /// Run the task processor loop in the main process.
    /// 
    /// IMPORTANT: Agent MUST run in a SEPARATE coroutine (tokio::spawn) to avoid deadlock.
    /// 
    /// When Agent executes tools like schedule_task, cancel_task, or list_tasks, it calls
    /// back to TaskManager which sends messages through this TaskProcessor's channel.
    /// 
    /// If Agent were on the same thread as TaskProcessor:
    /// 1. TaskProcessor sends task → Agent starts executing
    /// 2. Agent uses schedule_task tool → sends to TaskProcessor
    /// 3. TaskProcessor is BLOCKED waiting for Agent to finish
    /// 4. Agent is BLOCKED waiting for TaskProcessor to schedule new task
    /// 5. DEADLOCK!
    /// 
    /// By running Agent in a separate coroutine, both can proceed concurrently.
    pub async fn run(mut self, mut agent: crate::agent::Agent, tx: mpsc::Sender<String>) {
        // Agent worker in SEPARATE coroutine - avoids deadlock when Agent uses tools
        // that call back to TaskManager (schedule_task, cancel_task, list_tasks)
        let (exec_tx, mut exec_rx) = mpsc::channel::<Task>(32);
        tokio::spawn(async move {
            while let Some(task) = exec_rx.recv().await {
                log::info!("Executing task {}", task.id);
                let response = agent.send_message(task.payload).await;
                tx.send(response).await.expect("Failed to send to channel");
            }
        });

        let mut task_queue: BinaryHeap<Task> = BinaryHeap::new();
        loop {
            // Calculate sleep duration to next deadline
            // (Tokio sleep requires a std::time::Duration)
            let sleep_duration = match task_queue.peek() {
                Some(task) => {
                    let now = Utc::now();
                    if task.deadline <= now {
                        std::time::Duration::ZERO
                    } else {
                        (task.deadline - now)
                            .to_std()
                            .unwrap_or(std::time::Duration::ZERO)
                    }
                }
                None => std::time::Duration::MAX,
            };

            tokio::select! {
                // Branch 1: New task or cancellation arrives
                Some(msg) = self.rx.recv() => {
                    match msg {
                        TaskCommand::Schedule(task) => {
                            log::info!("Task #{} scheduled for {:?}", task.id, task.deadline);
                            task_queue.push(task);
                        }
                        TaskCommand::Cancel(task_id, reply_tx) => {
                            let mut found = false;
                            let mut new_queue = BinaryHeap::new();
                            for task in task_queue.drain() {
                                if task.id == task_id {
                                    found = true;
                                } else {
                                    new_queue.push(task);
                                }
                            }
                            task_queue = new_queue;

                            if found {
                                reply_tx.send(Ok(())).unwrap();
                            } else {
                                reply_tx.send(Err(CancelError::NotFound)).unwrap();
                            }
                        }
                        TaskCommand::ListTasks(reply_tx) => {
                            let mut tasks: Vec<_> = task_queue.iter().cloned().collect();
                            tasks.sort();
                            reply_tx.send(tasks).unwrap();
                        }
                    }
                },

                // Branch 2: Timer fires (next deadline reached)
                _ = sleep(sleep_duration) => {
                    // Process all ready tasks
                    while let Some(task) = task_queue.peek() {
                        if task.is_ready() {
                            let task = task_queue.pop().unwrap();
                            exec_tx.send(task).await.expect("Agent worker coroutine died");
                        } else {
                            break; // Next task not ready yet
                        }
                    }
                },

                // Branch 3: Channel closed
                else => {
                    // Process remaining tasks before exiting
                    while let Some(task) = task_queue.pop() {
                       exec_tx.send(task).await.expect("Agent worker coroutine died");
                    }
                    break;
                },
            }
        }
    }
}

/// Create the task management channel pair.
/// Returns (TaskManager, TaskProcessor) where:
/// - TaskManager is used to schedule/cancel tasks (runs in spawn)
/// - TaskProcessor processes tasks one at a time in main loop
pub fn create_task_channel() -> (TaskManager, TaskProcessor) {
    let (tx, rx) = mpsc::channel::<TaskCommand>(100);
    (TaskManager::new(tx), TaskProcessor::new(rx))
}
