use chrono::Utc;
use std::collections::BinaryHeap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Notify};
use tokio::time::sleep;

use super::{CancelError, Task, TaskCommand, TaskSaveError};

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

    /// Run the task processor loop, handling task scheduling, cancellation, and persistence.
    ///
    /// This async loop:
    /// 1. Receives commands (schedule, cancel, list) via `task_rx` channel
    /// 2. Sends ready tasks to the agent when their deadline is reached
    /// 3. Persists tasks to `tasks.json` on graceful shutdown
    ///
    /// Must be run in a separate coroutine (tokio::spawn) to allow concurrent task management.
    pub async fn run(mut self, agent_tx: mpsc::Sender<Task>, shutdown_signal: Arc<Notify>) {
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
