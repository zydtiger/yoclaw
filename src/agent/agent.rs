use reqwest::Client;
use serde_json::{json, Value};

use crate::agent::{tools, Agent, FinishReason, Message, Response, Role};
use crate::channels::{ChannelResponse, ResponseStatus};

const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");

impl Agent {
    pub async fn new(
        agent_config: &crate::config::AgentConfig,
        environment: std::collections::HashMap<String, String>,
        task_manager: std::sync::Arc<crate::tasks::TaskManager>,
        memory_store: crate::agent::MemoryStore,
        embedding: crate::agent::Embedding,
        channel_tx: tokio::sync::mpsc::Sender<ChannelResponse>,
    ) -> Result<Self, url::ParseError> {
        let parsed_url = match url::Url::parse(&agent_config.openai_api_base_url) {
            Ok(url) => url,
            Err(e) => {
                log::error!("Failed to parse API URL: {}", e);
                return Err(e);
            }
        };
        let parsed_url = parsed_url.join("chat/completions")?;

        let mut system_prompt = format!("{}\n\n{}", SYSTEM_PROMPT, agent_config.system_prompt);

        let mut skill_store = crate::agent::skills::SkillStore::default();
        // Load Anthropic-compatible skills context
        match crate::agent::skills::SkillStore::load_skills().await {
            Ok(loaded_store) => {
                let skills_ctx = loaded_store.get_context();
                if !skills_ctx.is_empty() {
                    system_prompt.push_str("\n\n");
                    system_prompt.push_str(&skills_ctx);
                }
                skill_store = loaded_store;
            }
            Err(e) => {
                log::warn!("Failed to load skills context: {}", e);
            }
        }

        Ok(Self {
            api_url: parsed_url,
            api_key: agent_config.openai_api_key.clone(),
            model: agent_config.openai_model.clone(),
            debug_mode: agent_config.debug_mode,

            environment,
            skill_store,
            tools: tools::get_all_tools(),
            client: Client::new(),
            messages: vec![Message::new(Role::System, system_prompt)],
            task_manager,
            memory_store,
            embedding,
            channel_tx,
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

    pub async fn start_task(&mut self, task_id: crate::tasks::TaskId, content: String) -> String {
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
                        // Tool-call progress is noisy for normal runs, so only surface it in debug mode.
                        if self.debug_mode {
                            let progress_msg = ChannelResponse {
                                task_id,
                                payload: format!(
                                    "🔧 Calling `{}` with args:\n```json\n{}\n```",
                                    tool_call.function.name, tool_call.function.arguments
                                ),
                                status: ResponseStatus::Continue,
                            };
                            if let Err(e) = self.channel_tx.send(progress_msg).await {
                                log::error!("Failed to send updates: {}", e);
                            }
                        }

                        log::info!("Calling tool: {}", tool_call.function.name);
                        let tool_result = tool_call
                            .execute(
                                &self.environment,
                                &self.skill_store,
                                self.task_manager.clone(),
                                &self.embedding,
                                &self.memory_store,
                            )
                            .await;

                        let message = Message::new(Role::Tool, tool_result)
                            .with_name(tool_call.function.name.clone())
                            .with_tool_call_id(tool_call.id.clone());
                        self.messages.push(message);
                    }
                }
                _ => {
                    // Standard Text Response
                    let content = choice.message.content.clone();
                    if let Some(s) = content {
                        if self.debug_mode {
                            let debug_info = json!({
                                "timings": response.timings,
                                "usage": response.usage
                            });
                            let formatted = serde_json::to_string_pretty(&debug_info)
                                .unwrap_or_else(|_e| {
                                    format!("{{\"error\":\"Failed to format debug info\"}}")
                                });
                            return format!("{}\n\n{}", s, formatted);
                        }
                        return s;
                    } else {
                        return "Error: Empty message".into();
                    }
                }
            };
        }
    }
}
