use chrono::Duration;
use chrono::Utc;
use std::collections::BinaryHeap;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::tasks::{CancelError, Task, TaskId};

/// TaskManager provides the interface for scheduling and cancelling tasks.
/// It runs in a separate tokio::spawn and sends tasks to the main TaskProcessor.
pub struct TaskManager {
    tx: mpsc::Sender<Task>,
}

impl TaskManager {
    pub fn new(tx: mpsc::Sender<Task>) -> Self {
        Self { tx }
    }

    /// Schedule a task to run immediately
    pub async fn schedule_task(
        &self,
        payload: String,
    ) -> Result<TaskId, mpsc::error::SendError<Task>> {
        let task = Task::new(payload);
        let task_id = task.id;
        self.tx.send(task).await?;
        Ok(task_id)
    }

    /// Schedule a task to run after a delay
    pub async fn schedule_task_in(
        &self,
        payload: String,
        delay: Duration,
    ) -> Result<TaskId, mpsc::error::SendError<Task>> {
        let task = Task::scheduled(payload, delay);
        let task_id = task.id;
        self.tx.send(task).await?;
        Ok(task_id)
    }

    /// Cancel a pending task by its ID
    pub async fn cancel_task(&self, _task_id: TaskId) -> Result<(), CancelError> {
        // TODO: Implement cancellation logic for tasks in the priority queue
        Ok(())
    }
}

/// TaskProcessor handles the actual task queue and processing in the main loop.
/// Uses a priority queue (BinaryHeap) to manage tasks by deadline.
pub struct TaskProcessor {
    rx: mpsc::Receiver<Task>,
}

impl TaskProcessor {
    pub fn new(rx: mpsc::Receiver<Task>) -> Self {
        Self { rx }
    }

    /// Run the task processor loop in the main process.
    /// Processes tasks one-by-one using the shared Agent (no cloning).
    /// Uses tokio::select! to handle both new arrivals and scheduled deadlines.
    pub async fn run(mut self, agent: &mut crate::agent::Agent) {
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
                // Branch 1: New task arrives
                Some(task) = self.rx.recv() => {
                    log::info!("Task #{} scheduled for {:?}", task.id, task.deadline);
                    task_queue.push(task);
                },

                // Branch 2: Timer fires (next deadline reached)
                _ = sleep(sleep_duration) => {
                    // Process all ready tasks
                    while let Some(task) = task_queue.peek() {
                        if task.is_ready() {
                            let task = task_queue.pop().unwrap();
                            log::info!("Processing scheduled task #{}: {}", task.id, task.payload);
                            let response = agent.send_message(task.payload).await;
                            log::info!("Task #{} response: {}", task.id, response);
                        } else {
                            break; // Next task not ready yet
                        }
                    }
                },

                // Branch 3: Channel closed
                else => {
                    // Process remaining tasks before exiting
                    while let Some(task) = task_queue.pop() {
                        log::info!("Processing remaining task #{}: {}", task.id, task.payload);
                        let response = agent.send_message(task.payload).await;
                        log::info!("Task #{} response: {}", task.id, response);
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
    let (tx, rx) = mpsc::channel::<Task>(100);
    (TaskManager::new(tx), TaskProcessor::new(rx))
}
