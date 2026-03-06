use tokio::sync::mpsc;

use crate::tasks::{Task, TaskId};

/// TaskManager provides the interface for scheduling and cancelling tasks.
/// It runs in a separate tokio::spawn and sends tasks to the main TaskProcessor.
pub struct TaskManager {
    tx: mpsc::Sender<Task>,
}

impl TaskManager {
    pub fn new(tx: mpsc::Sender<Task>) -> Self {
        Self { tx }
    }

    /// Schedule a new task, returns the assigned TaskId.
    pub async fn schedule_task(
        &self,
        payload: String,
    ) -> Result<TaskId, mpsc::error::SendError<Task>> {
        let task = Task::new(payload);
        let task_id = task.id;
        self.tx.send(task).await?;
        Ok(task_id)
    }
}

/// TaskProcessor handles the actual task queue and processing in the main loop.
pub struct TaskProcessor {
    rx: mpsc::Receiver<Task>,
}

impl TaskProcessor {
    pub fn new(rx: mpsc::Receiver<Task>) -> Self {
        Self { rx }
    }

    /// Run the task processor loop in the main process.
    /// Processes tasks one-by-one using the shared Agent (no cloning).
    pub async fn run(mut self, agent: &mut crate::agent::Agent) {
        while let Some(task) = self.rx.recv().await {
            log::info!("Processing task #{}: {}", task.id, task.payload);
            let response = agent.send_message(task.payload).await;
            log::info!("Task #{} response: {}", task.id, response);
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
