mod manager;
mod processor;

use chrono::{DateTime, Duration, Local, Utc};
use serde::{Deserialize, Serialize, Serializer};
use std::sync::atomic::{self, AtomicU64};
use tokio::sync::{mpsc, oneshot};

pub use manager::TaskManager;
pub use processor::TaskProcessor;

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

/// Unique identifier for a task
pub type TaskId = u64;

/// Static counter for generating unique TaskIds
pub static TASK_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub enum TaskCommand {
    Schedule(Task),
    Cancel(TaskId, oneshot::Sender<Result<(), CancelError>>),
    ListTasks(oneshot::Sender<Vec<Task>>),
}

/// A task to be processed by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub payload: String,

    #[serde(serialize_with = "serialize_to_local")]
    pub deadline: DateTime<Utc>,
}

fn serialize_to_local<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let local_date: DateTime<Local> = DateTime::from(*date);
    serializer.serialize_str(&local_date.to_rfc3339())
}

impl Task {
    /// Create a new task with the given payload (immediate execution)
    pub fn new(payload: String) -> Self {
        let id = TASK_ID_COUNTER.fetch_add(1, atomic::Ordering::Relaxed);
        Self {
            id,
            payload,
            deadline: Utc::now(),
        }
    }

    /// Create a scheduled task with a delay
    pub fn scheduled(payload: String, delay: Duration) -> Self {
        let id = TASK_ID_COUNTER.fetch_add(1, atomic::Ordering::Relaxed);
        Self {
            id,
            payload,
            deadline: Utc::now() + delay,
        }
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
