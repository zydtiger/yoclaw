use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};

use crate::channels::ChannelResponse;

mod agent;
mod embedding;
mod memory;
mod message;
pub mod skills;
mod tools;

use tools::{Tool, ToolCall};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    pub choices: Vec<Choice>,
    pub created: i64,
    pub id: String,
    pub model: String,
    pub object: String,
    pub system_fingerprint: String,
    pub timings: GenerationMetrics,
    pub usage: UsageMetrics,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Choice {
    pub finish_reason: FinishReason,
    pub index: i32,
    pub message: Message,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GenerationMetrics {
    pub cache_n: i32,
    pub predicted_ms: f32,
    pub predicted_n: i32,
    pub predicted_per_second: f32,
    pub predicted_per_token_ms: f32,
    pub prompt_ms: f32,
    pub prompt_n: i32,
    pub prompt_per_second: f32,
    pub prompt_per_token_ms: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UsageMetrics {
    pub completion_tokens: u32,
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,

    #[serde(rename = "content_filter")]
    ContentFilter,

    #[serde(rename = "tool_calls")]
    ToolCalls,

    #[serde(other)] // Catch-all for forward compatibility (e.g., "null" or unknown)
    Unknown,
}

/// Represents a message role in the conversation.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    System,
    Tool,
    Assistant,
}

/// Represents a message in the conversation.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: Role,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Default)]
pub struct MessageHistory {
    entries: Vec<Message>,
    pub total_tokens: u32,
}

impl MessageHistory {
    pub fn new(entries: Vec<Message>) -> Self {
        Self {
            entries,
            total_tokens: 0,
        }
    }
}

impl Deref for MessageHistory {
    type Target = Vec<Message>;

    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl DerefMut for MessageHistory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

impl Serialize for MessageHistory {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.entries.serialize(serializer)
    }
}

/// Agent struct
#[derive(Debug)]
pub struct Agent {
    // Core LLM related
    pub api_url: Url,
    pub api_key: String,
    pub model: String,
    pub context_size: u32,
    pub debug_mode: bool,

    // Tool call related
    pub environment: std::collections::HashMap<String, String>,
    pub skill_store: crate::agent::skills::SkillStore,
    pub tools: Vec<Tool>,
    pub client: Client,
    pub messages: MessageHistory,
    pub task_manager: std::sync::Arc<crate::tasks::TaskManager>,
    pub memory_store: MemoryStore,
    pub embedding: Embedding,

    // Channel sender for progress updates during tool calls
    pub channel_tx: tokio::sync::mpsc::Sender<ChannelResponse>,
}

/// Memory store struct for vector database
#[derive(Debug)]
pub struct MemoryStore {
    conn: rusqlite::Connection,
}

/// Embedding client
#[derive(Debug)]
pub struct Embedding {
    pub api_url: Url,
    pub api_key: String,
    pub model: String,
    pub client: Client,
}
