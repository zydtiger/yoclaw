use chrono::Duration;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use super::{
    CancelError, ScheduleTaskError, Task, TaskCommand, TaskId, TaskRepeat, TaskRouteBinding,
    TaskRouter,
};

/// TaskManager provides the interface for scheduling and cancelling tasks.
/// It runs in a separate tokio::spawn and sends tasks to the main TaskProcessor.
#[derive(Debug)]
pub struct TaskManager {
    task_tx: mpsc::Sender<TaskCommand>,
    task_router: Arc<TaskRouter>,
}

impl TaskManager {
    pub fn new(tx: mpsc::Sender<TaskCommand>, task_router: Arc<TaskRouter>) -> Self {
        Self {
            task_tx: tx,
            task_router,
        }
    }

    /// Schedule a task with optional delay, repeat interval, and route binding.
    pub async fn schedule_task(
        &self,
        payload: String,
        delay: Option<Duration>,
        repeat: Option<TaskRepeat>,
        route: TaskRouteBinding,
    ) -> Result<TaskId, ScheduleTaskError> {
        let task = Task::scheduled(payload, delay, repeat);
        let task_id = task.id;
        let route_chat_id = match route {
            TaskRouteBinding::ChatId(chat_id) => Some(chat_id),
            TaskRouteBinding::Inherit(parent_task_id) => Some(
                self.task_router
                    .get(&parent_task_id)
                    .await
                    .ok_or(ScheduleTaskError::MissingRoute(parent_task_id))?,
            ),
        };

        if let Some(chat_id) = route_chat_id {
            self.task_router.insert(task_id, chat_id).await;
        }

        if self
            .task_tx
            .send(TaskCommand::Schedule(task))
            .await
            .is_err()
        {
            self.task_router.remove(&task_id).await;
            return Err(ScheduleTaskError::QueueClosed);
        }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn schedule_task_registers_chat_route_before_enqueue() {
        let (task_tx, mut task_rx) = mpsc::channel(1);
        let task_router = Arc::new(TaskRouter::default());
        let manager = TaskManager::new(task_tx, task_router.clone());

        let task_id = manager
            .schedule_task(
                "ping".to_string(),
                None,
                None,
                TaskRouteBinding::ChatId("chat-1".to_string()),
            )
            .await
            .expect("schedule should succeed");

        let scheduled_task = match task_rx.recv().await {
            Some(TaskCommand::Schedule(task)) => task,
            _ => panic!("expected scheduled task"),
        };

        assert_eq!(scheduled_task.id, task_id);
        assert_eq!(task_router.get(&task_id).await.as_deref(), Some("chat-1"));
    }

    #[tokio::test]
    async fn schedule_task_inherit_fails_without_parent_route() {
        let (task_tx, _task_rx) = mpsc::channel(1);
        let task_router = Arc::new(TaskRouter::default());
        let manager = TaskManager::new(task_tx, task_router);
        let parent_task_id = uuid::Uuid::now_v7();

        let result = manager
            .schedule_task(
                "ping".to_string(),
                Some(Duration::seconds(5)),
                None,
                TaskRouteBinding::Inherit(parent_task_id),
            )
            .await;

        assert_eq!(result, Err(ScheduleTaskError::MissingRoute(parent_task_id)));
    }

    #[tokio::test]
    async fn schedule_task_rolls_back_route_when_queue_is_closed() {
        let (task_tx, task_rx) = mpsc::channel(1);
        drop(task_rx);

        let task_router = Arc::new(TaskRouter::default());
        let manager = TaskManager::new(task_tx, task_router.clone());

        let result = manager
            .schedule_task(
                "ping".to_string(),
                None,
                None,
                TaskRouteBinding::ChatId("chat-1".to_string()),
            )
            .await;

        assert_eq!(result, Err(ScheduleTaskError::QueueClosed));
        assert_eq!(task_router.len().await, 0);
    }
}
