pub mod task_manager;

use chrono::{DateTime, Duration, Local, Utc};
use serde::ser::{Serialize, SerializeStruct, Serializer};
use std::sync::atomic::{self, AtomicU64};
use tokio::sync::oneshot;

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
#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub payload: String,
    pub deadline: DateTime<Utc>,
}

impl Serialize for Task {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Task", 3)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("payload", &self.payload)?;

        // Convert Utc to Local time for serialization
        let local_deadline = self.deadline.with_timezone(&Local);
        state.serialize_field("deadline", &local_deadline.to_rfc3339())?;

        state.end()
    }
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
