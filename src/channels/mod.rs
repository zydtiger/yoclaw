use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

mod command;
mod handler;
pub mod telegram;

use crate::tasks::{TaskId, TaskRouter};

/// A bot command that can be registered with a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCommand {
    pub command: String,
    pub description: String,
}

/// Manages channel commands.
pub struct CommandManager {
    pub commands: Vec<BotCommand>,
}

/// A generic message received from any channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub channel_id: String,
    pub chat_id: String,
    pub message_id: i64,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResponseStatus {
    Continue,
    Terminate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelResponse {
    pub task_id: TaskId,
    pub payload: String,
    pub status: ResponseStatus,
}

/// A generic trait for all communication channels.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Send a message specifically to a recipient via this channel.
    async fn send_message(
        &self,
        recipient_id: &str,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Poll or receive new messages from the channel.
    async fn receive_messages(
        &self,
    ) -> Result<Vec<ChannelMessage>, Box<dyn std::error::Error + Send + Sync>>;

    /// Add an emoji reaction to a message.
    async fn react_with_emoji(
        &self,
        chat_id: &str,
        message_id: i64,
        emoji: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Register bot commands.
    async fn register_commands(
        &self,
        commands: Vec<BotCommand>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Handler for channel listening.
#[derive(Clone)]
pub struct ChannelHandler {
    pub channel: Arc<dyn Channel>,
    pub allowed_users: Vec<String>,
    pub recv_confirm: Option<String>,
    pub task_router: Arc<TaskRouter>,
}
