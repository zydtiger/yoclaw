use serde::{Deserialize, Serialize};

pub mod telegram;

/// A generic message received from any channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub channel_id: String,
    pub chat_id: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub text: String,
}

/// A generic trait for all communication channels.
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
}
