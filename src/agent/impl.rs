use reqwest::{Client, IntoUrl};
use serde_json::{json, Value};

use crate::agent::{tools, Agent, FinishReason, Message, Response, Role};

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

impl Agent {
    pub fn new(
        api_url: impl IntoUrl,
        api_key: &str,
        model: &str,
        system_prompt: &str,
        task_manager: std::sync::Arc<crate::tasks::task_manager::TaskManager>,
    ) -> Result<Self, reqwest::Error> {
        let parsed_url = match api_url.into_url() {
            Ok(url) => url.join("chat/completions"),
            Err(e) => {
                log::error!("Failed to parse API URL: {}", e);
                return Err(e);
            }
        };

        Ok(Self {
            api_url: parsed_url,
            api_key: api_key.to_string(),
            model: model.to_string(),

            tools: tools::get_all_tools(),
            client: Client::new(),
            messages: vec![Message::new(Role::System, system_prompt.to_string())],
            task_manager,
        })
    }

    /// Call an OpenAI-compatible API with tool support.
    async fn call(&self) -> Result<Response, String> {
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

        match result.get("error") {
            Some(e) => return Err(e.to_string()),
            None => match serde_json::from_value::<Response>(result) {
                Err(e) => return Err(e.to_string()),
                Ok(res) => Ok(res),
            },
        }
    }

    pub async fn send_message(&mut self, content: String) -> String {
        self.messages.push(Message::new(Role::User, content));

        loop {
            let response = match self.call().await {
                Ok(res) => res,
                Err(e) => return format!("Error: {e}"),
            };

            // NOTE: Only process first choice, assumed to be the only one
            let choice = match response.choices.first() {
                Some(c) => c,
                None => return "Error: Response choices is empty".into(),
            };

            self.messages.push(choice.message.clone());

            match &choice.finish_reason {
                FinishReason::ToolCalls => {
                    let tool_calls = match &choice.message.tool_calls {
                        Some(tc) if !tc.is_empty() => tc,
                        _ => return "Error: Tool call expected but not found".into(),
                    };

                    for tool_call in tool_calls {
                        log::info!("Calling tool: {}", tool_call.function.name);
                        let tool_result = tool_call.execute(self.task_manager.clone()).await;

                        let message = Message::new(Role::Tool, tool_result)
                            .with_name(tool_call.function.name.clone())
                            .with_tool_call_id(tool_call.id.clone());
                        self.messages.push(message);
                    }
                }
                _ => {
                    // Standard Text Response
                    return choice
                        .message
                        .content
                        .clone()
                        .unwrap_or_else(|| "Error: Empty message".into());
                }
            };
        }
    }
}
