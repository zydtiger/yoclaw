use chrono::Duration;
use tokio::sync::{mpsc, oneshot};

use super::{CancelError, Task, TaskCommand, TaskId, TaskRepeat};

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
        let task = Task::scheduled(payload, delay, None);
        let task_id = task.id;
        self.task_tx.send(TaskCommand::Schedule(task)).await?;
        Ok(task_id)
    }

    /// Schedule a task to run after a delay and repeat on a fixed interval.
    pub async fn schedule_repeating_task_in(
        &self,
        payload: String,
        delay: Duration,
        repeat: TaskRepeat,
    ) -> Result<TaskId, mpsc::error::SendError<TaskCommand>> {
        let task = Task::scheduled(payload, delay, Some(repeat));
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
