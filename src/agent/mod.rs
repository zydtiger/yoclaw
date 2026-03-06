use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::tools::{Tool, ToolCall};

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
    pub completion_tokens: i32,
    pub prompt_tokens: i32,
    pub total_tokens: i32,
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

impl Message {
    pub fn new(role: Role, content: String) -> Self {
        Self {
            role,
            content: Some(content),
            reasoning_content: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_tool_call_id(mut self, tool_call_id: String) -> Self {
        self.tool_call_id = Some(tool_call_id);
        self
    }
}

pub struct Agent {
    pub api_url: Url,
    pub api_key: String,
    pub model: String,
    pub tools: Vec<Tool>,

    /// Self managed
    pub client: Client,
    pub messages: Vec<Message>,
}

impl Agent {
    pub fn new(api_url: Url, api_key: String, model: String, tools: Vec<Tool>) -> Self {
        Self {
            api_url,
            api_key,
            model,
            tools,

            client: Client::new(),
            messages: vec![],
        }
    }

    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Call an OpenAI-compatible API with tool support.
    pub async fn call(&self) -> Result<Value, String> {
        let payload = json!({
            "model": &self.model,
            "messages": &self.messages,
            "tools": &self.tools,
            "tool_choice": "auto"
        });

        let response = self
            .client
            .post(self.api_url.clone())
            .header("Authorization", format!("Bearer {}", &self.api_key))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let result = response.json::<Value>().await.map_err(|e| e.to_string())?;

        Ok(result)
    }
}
