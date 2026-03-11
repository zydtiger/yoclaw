use chrono::Duration;
use chrono::Utc;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot, Notify};
use tokio::time::sleep;

use super::{CancelError, Task, TaskCommand, TaskId, TaskSaveError};

/// TaskManager provides the interface for scheduling and cancelling tasks.
/// It runs in a separate tokio::spawn and sends tasks to the main TaskProcessor.
#[derive(Debug)]
pub struct TaskManager {
    task_tx: mpsc::Sender<TaskCommand>,
}

impl TaskManager {
    pub fn new(tx: mpsc::Sender<TaskCommand>) -> Self {
        Self { task_tx: tx }
    }

    /// Schedule a task to run immediately
    pub async fn schedule_task(
        &self,
        payload: String,
    ) -> Result<TaskId, mpsc::error::SendError<TaskCommand>> {
        let task = Task::new(payload);
        let task_id = task.id;
        self.task_tx.send(TaskCommand::Schedule(task)).await?;
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
        self.task_tx.send(TaskCommand::Schedule(task)).await?;
        Ok(task_id)
    }

    /// Cancel a pending task by its ID
    pub async fn cancel_task(&self, task_id: TaskId) -> Result<(), CancelError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if self
            .task_tx
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
            .task_tx
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
    task_rx: mpsc::Receiver<TaskCommand>,
    pending_tasks: BinaryHeap<Task>,
}

impl TaskProcessor {
    /// Create a new TaskProcessor, loading any persisted tasks from tasks.json
    pub async fn new(rx: mpsc::Receiver<TaskCommand>) -> Self {
        let mut pending_tasks = BinaryHeap::new();

        // Load persisted tasks from tasks.json
        match Self::load_tasks().await {
            Ok(tasks) => {
                if !tasks.is_empty() {
                    log::info!(
                        "TaskProcessor: Loaded {} persisted task(s) from tasks.json",
                        tasks.len()
                    );
                    // Add loaded tasks to the pending queue
                    for task in tasks {
                        pending_tasks.push(task);
                    }
                }
            }
            Err(e) => {
                log::warn!("TaskProcessor: Failed to load persisted tasks: {}", e);
            }
        }

        Self {
            task_rx: rx,
            pending_tasks,
        }
    }

    /// Run the task processor loop in the main process with signal handling.
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
    pub async fn run(
        mut self,
        mut agent: crate::agent::Agent,
        channel_tx: mpsc::Sender<String>,
        shutdown_signal: Arc<Notify>,
    ) {
        // Agent worker in SEPARATE coroutine - avoids deadlock when Agent uses tools
        // that call back to TaskManager (schedule_task, cancel_task, list_tasks)
        let (agent_tx, mut agent_rx) = mpsc::channel::<Task>(32);
        tokio::spawn(async move {
            while let Some(task) = agent_rx.recv().await {
                log::info!("Executing task {}", task.id);
                let response = agent.send_message(task.payload).await;
                channel_tx
                    .send(response)
                    .await
                    .expect("Failed to send to channel");
            }
        });

        loop {
            // Calculate sleep duration to next deadline
            // (Tokio sleep requires a std::time::Duration)
            let sleep_duration = match self.pending_tasks.peek() {
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
                Some(msg) = self.task_rx.recv() => {
                    match msg {
                        TaskCommand::Schedule(task) => {
                            log::info!("Task #{} scheduled for {:?}", task.id, task.deadline);
                            self.pending_tasks.push(task);
                        }
                        TaskCommand::Cancel(task_id, reply_tx) => {
                            let mut found = false;
                            let mut new_queue = BinaryHeap::new();
                            for task in self.pending_tasks.drain() {
                                if task.id == task_id {
                                    found = true;
                                } else {
                                    new_queue.push(task);
                                }
                            }
                            self.pending_tasks = new_queue;

                            if found {
                                reply_tx.send(Ok(())).unwrap();
                            } else {
                                reply_tx.send(Err(CancelError::NotFound)).unwrap();
                            }
                        }
                        TaskCommand::ListTasks(reply_tx) => {
                            let mut tasks: Vec<_> = self.pending_tasks.iter().cloned().collect();
                            tasks.sort();
                            reply_tx.send(tasks).unwrap();
                        }
                    }
                },

                // Branch 2: Timer fires (next deadline reached)
                _ = sleep(sleep_duration) => {
                    // Process all ready tasks
                    while let Some(task) = self.pending_tasks.peek() {
                        if task.is_ready() {
                            let task = self.pending_tasks.pop().unwrap();
                            agent_tx.send(task).await.expect("Agent worker coroutine died");
                        } else {
                            break; // Next task not ready yet
                        }
                    }
                },

                // Branch 3: Graceful shutdown when SIGTERM is received
                _ = shutdown_signal.notified() => {
                    log::info!("Shutdown requested, saving {} pending task(s)", self.pending_tasks.len());
                    let pending: Vec<Task> = self.pending_tasks.iter().cloned().collect();
                    if let Err(e) = Self::save_tasks(&pending).await {
                        log::error!("Failed to save pending tasks: {}", e);
                    } else {
                        log::info!("Successfully saved {} pending task(s) to tasks.json", pending.len());
                    }
                    break;
                }

                // Branch 4: Channel closed (graceful shutdown)
                else => {
                    log::info!("TaskProcessor shutting down, saving {} pending task(s)", self.pending_tasks.len());
                    // Save pending tasks before exiting
                    let pending: Vec<Task> = self.pending_tasks.iter().cloned().collect();
                    if let Err(e) = Self::save_tasks(&pending).await {
                        log::error!("Failed to save pending tasks: {}", e);
                    } else {
                        log::info!("Successfully saved {} pending task(s) to tasks.json", pending.len());
                    }
                    break;
                },
            }
        }
    }

    /// Get the path to the tasks.json file used for persisting tasks.
    ///
    /// This file is stored in the CONFIG_DIR and contains all pending tasks
    /// that need to be loaded on application restart.
    pub fn get_tasks_path() -> PathBuf {
        PathBuf::from(&*crate::globals::CONFIG_DIR).join("tasks.json")
    }

    /// Save tasks to a JSON file
    pub async fn save_tasks(tasks: &[Task]) -> Result<(), TaskSaveError> {
        let json =
            serde_json::to_string_pretty(tasks).map_err(|e| TaskSaveError::InvalidFormat(e))?;

        let file_path = Self::get_tasks_path();
        let mut file = File::create(file_path)
            .await
            .map_err(|e| TaskSaveError::FsError(e))?;

        file.write_all(json.as_bytes())
            .await
            .map_err(|e| TaskSaveError::FsError(e))?;

        Ok(())
    }

    /// Load tasks from a JSON file
    pub async fn load_tasks() -> Result<Vec<Task>, TaskSaveError> {
        let file_path = Self::get_tasks_path();

        if !file_path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(file_path)
            .await
            .map_err(|e| TaskSaveError::FsError(e))?;

        let mut tasks: Vec<Task> =
            serde_json::from_str(&content).map_err(|e| TaskSaveError::InvalidFormat(e))?;

        // Reset ids for tasks
        tasks.iter_mut().enumerate().for_each(|(i, task)| {
            task.id = i as u64;
        });

        super::TASK_ID_COUNTER.store(tasks.len() as u64, std::sync::atomic::Ordering::Relaxed);

        Ok(tasks)
    }
}

/// Create the task management channel pair.
/// Returns (TaskManager, TaskProcessor) where:
/// - TaskManager is used to schedule/cancel tasks (runs in spawn)
/// - TaskProcessor processes tasks one at a time in main loop
///
/// Note: TaskProcessor::new is async because it loads persisted tasks from disk
pub async fn create_task_channel() -> (TaskManager, TaskProcessor) {
    let (tx, rx) = mpsc::channel::<TaskCommand>(100);
    (TaskManager::new(tx), TaskProcessor::new(rx).await)
}
