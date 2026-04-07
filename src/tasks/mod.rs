mod manager;
mod processor;
mod router;

use chrono::{DateTime, Duration, Local, Utc};
use serde::{Deserialize, Serialize, Serializer};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

pub use manager::TaskManager;
pub use processor::TaskProcessor;
pub use router::TaskRouter;

/// Create the task management channel pair.
/// Returns (TaskManager, TaskProcessor) where:
/// - TaskManager is the send-side API for scheduling/canceling/listing tasks
/// - TaskProcessor is the receive-side worker that should run in a spawned task
///
/// Note: TaskProcessor::new is async because it loads persisted tasks from disk
pub async fn create_task_channel() -> (TaskManager, TaskProcessor, Arc<TaskRouter>) {
    let (tx, rx) = mpsc::channel::<TaskCommand>(100);
    let task_router = Arc::new(TaskRouter::new().await);
    (
        TaskManager::new(tx, task_router.clone()),
        TaskProcessor::new(rx, task_router.clone()).await,
        task_router,
    )
}

/// Unique identifier for a task
pub type TaskId = uuid::Uuid;

#[derive(Debug, Clone)]
pub enum TaskRouteBinding {
    ChatId(String),
    Inherit(TaskId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleTaskError {
    MissingRoute(TaskId),
    QueueClosed,
}

impl std::fmt::Display for ScheduleTaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingRoute(task_id) => write!(f, "missing route for task {}", task_id),
            Self::QueueClosed => write!(f, "task processor channel is closed"),
        }
    }
}

impl std::error::Error for ScheduleTaskError {}

pub enum TaskCommand {
    Schedule(Task),
    Cancel(TaskId, oneshot::Sender<Result<(), CancelError>>),
    ListTasks(oneshot::Sender<Vec<Task>>),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskRepeat {
    Daily,
    Weekly,
}

impl TaskRepeat {
    pub fn next_deadline(self, deadline: DateTime<Utc>) -> DateTime<Utc> {
        match self {
            Self::Daily => deadline + Duration::days(1),
            Self::Weekly => deadline + Duration::weeks(1),
        }
    }
}

/// A task to be processed by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub payload: String,

    #[serde(serialize_with = "serialize_to_local")]
    pub deadline: DateTime<Utc>,

    #[serde(default)]
    pub repeat: Option<TaskRepeat>,
}

fn serialize_to_local<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let local_date: DateTime<Local> = DateTime::from(*date);
    serializer.serialize_str(&local_date.to_rfc3339())
}

impl Task {
    /// Create a task with an optional delay and repeat interval.
    pub fn scheduled(payload: String, delay: Option<Duration>, repeat: Option<TaskRepeat>) -> Self {
        Self {
            id: uuid::Uuid::now_v7(),
            payload,
            deadline: Utc::now() + delay.unwrap_or(Duration::zero()),
            repeat,
        }
    }

    /// Create the next instance of a repeating task using the original deadline as the anchor.
    pub fn next_recurrence(&self) -> Option<Self> {
        self.repeat.map(|repeat| Self {
            id: uuid::Uuid::now_v7(),
            payload: self.payload.clone(),
            deadline: repeat.next_deadline(self.deadline),
            repeat: Some(repeat),
        })
    }

    /// Check if the task is ready to execute
    pub fn is_ready(&self) -> bool {
        Utc::now() >= self.deadline
    }
}

// Equality: same ID means same task
impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Task {}

// Ordering: earlier deadline = higher priority (for min-heap)
impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Task {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering: earlier deadline has HIGHER priority
        other
            .deadline
            .cmp(&self.deadline)
            // Tie-breaker: smaller ID (created earlier) has HIGHER priority
            .then_with(|| other.id.cmp(&self.id))
    }
}

/// Error type for task cancellation failures
#[derive(Debug, Clone, PartialEq)]
pub enum CancelError {
    /// Task was already processed or not found
    NotFound,
}

impl std::fmt::Display for CancelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CancelError::NotFound => write!(f, "task not found"),
        }
    }
}

impl std::error::Error for CancelError {}

/// Error type for task persistence (save/load) failures
#[derive(Debug)]
pub enum TaskSaveError {
    /// Failed to serialize/deserialize JSON
    InvalidFormat(serde_json::Error),
    /// File system error (read/write)
    FsError(std::io::Error),
}

impl std::fmt::Display for TaskSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskSaveError::InvalidFormat(e) => write!(f, "Invalid task format: {}", e),
            TaskSaveError::FsError(e) => write!(f, "File system error: {}", e),
        }
    }
}

impl std::error::Error for TaskSaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            TaskSaveError::InvalidFormat(e) => Some(e),
            TaskSaveError::FsError(e) => Some(e),
        }
    }
}
