# Repo Memory

## Runtime Shape
- `src/main.rs` keeps the `Agent` on the main thread. This is intentional so agent state like message history stays single-owner and `!Send` state does not need to cross tasks.
- There are 4 long-lived async flows: shutdown signal listener, Telegram listener, Telegram sender, and `TaskProcessor`.
- `TaskProcessor` must stay separate from the main-thread agent. If task dispatch and task management share the same thread, agent tool calls back into `TaskManager` can deadlock.

## Channel Split
- Channel I/O is split on purpose:
  - `ChannelHandler::start_listening(...)` only polls incoming Telegram messages and schedules tasks.
  - `ChannelHandler::start_sending(...)` only drains `channel_rx` and sends outbound responses.
- Do not merge send and receive back into one `tokio::select!` loop. Telegram `getUpdates` uses long polling, which causes visible reply lag if sending shares the same loop.
- `ChannelHandler` shares `channel: Arc<dyn Channel>` and `task_routes: Arc<Mutex<HashMap<TaskId, String>>>` between listener and sender.

## Response Routing Contract
- Outbound channel messages use `channels::ChannelResponse` instead of tuples.
- `ChannelResponse` contains `task_id`, `payload`, and `status`.
- `ResponseStatus::Continue` is for intermediate progress updates, especially tool-call progress.
- `ResponseStatus::Terminate` marks the final reply for a task. Sender removes that task's route after successfully handling the response path.

## Shutdown Contract
- Shutdown broadcast only targets the listener and `TaskProcessor`.
- The sender does not listen for shutdown directly. It exits only when all `channel_tx` senders are dropped and `channel_rx` closes.
- Shutdown ordering in `main` matters:
  1. wait for listener to stop accepting new Telegram messages
  2. wait for `TaskProcessor` to exit and drop `agent_tx`
  3. main agent loop exits when `agent_rx` closes
  4. drop `agent` and `channel_tx`
  5. sender drains buffered responses, saves `routes.json`, and exits
- Keep `drop(agent)` and `drop(channel_tx)` before awaiting the sender. They are required so the sender can observe channel closure and finish cleanly.

## Task Model
- `tasks::Task` now includes `repeat: Option<TaskRepeat>`.
- Supported repeat values are `Daily` and `Weekly`.
- Repeating tasks reschedule from the original deadline, not from the time they finish running. Example: a weekly 9:00 AM task due March 13 creates the next occurrence for March 20 at 9:00 AM.
- `TaskProcessor` enqueues the next recurrence before dispatching the current due task.
- Persisted tasks are stored in `tasks.json`; channel routes are stored in `routes.json`.

## Tool Surface
- The built-in `schedule_task` tool accepts `payload`, `delay_seconds`, and optional `repeat` with values `daily` or `weekly`.
- `list_tasks` output now implicitly includes repeat metadata because it serializes the full `Task`.

## Practical Editing Notes
- If you change shutdown, preserve the invariant that route saving happens after the sender is done mutating `task_routes`.
- If you change task scheduling, preserve backward compatibility for persisted tasks by keeping `repeat` optional on deserialize.
- If you update architecture docs, `STRUCTURE.md` is the verbose source of truth; keep this file concise.
