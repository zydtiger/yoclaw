use std::sync::atomic::{AtomicU64, Ordering};

mod task_manager;

pub use task_manager::create_task_channel;

/// Unique identifier for a task
pub type TaskId = u64;

/// Static counter for generating unique TaskIds
pub static TASK_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A task to be processed by the agent
#[derive(Debug, Clone)]
pub struct Task {
    pub id: TaskId,
    pub payload: String,
}

impl Task {
    /// Create a new task with the given payload.
    /// The TaskId is automatically assigned.
    pub fn new(payload: String) -> Self {
        let id = TASK_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
        Self { id, payload }
    }
}
