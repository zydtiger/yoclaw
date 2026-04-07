use reqwest::Client;
use serde_json::{json, Value};

use crate::agent::{tools, Agent, FinishReason, Message, MessageHistory, Response, Role};
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
            context_size: agent_config.context_size,
            debug_mode: agent_config.debug_mode,

            environment,
            skill_store,
            tools: tools::get_all_tools(),
            client: Client::new(),
            messages: MessageHistory::new(vec![Message::new(Role::System, system_prompt)]),
            task_manager,
            memory_store,
            embedding,
            channel_tx,
        })
    }

    pub fn clear_history(&mut self) {
        self.messages.clear_preserving_system();
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
        let task_start_offset = self.messages.len();
        self.messages.push(Message::new(Role::User, content));

        loop {
            let response = match self.call().await {
                Ok(res) => res,
                Err(e) => {
                    let error = format!("Error: {e}");
                    self.messages
                        .compact_task_messages(task_start_offset, error.clone());
                    return error;
                }
            };
            self.messages.total_tokens = response.usage.total_tokens;

            // NOTE: Only process first choice, assumed to be the only one
            let choice = match response.choices.first() {
                Some(c) => c,
                None => {
                    let error = "Error: Response choices is empty".to_string();
                    self.messages
                        .compact_task_messages(task_start_offset, error.clone());
                    return error;
                }
            };

            let assistant_message = choice.message.clone();
            self.messages.push(assistant_message.clone());

            match &choice.finish_reason {
                FinishReason::ToolCalls => {
                    let tool_calls = match &assistant_message.tool_calls {
                        Some(tc) if !tc.is_empty() => tc,
                        _ => {
                            let error = "Error: Tool call expected but not found".to_string();
                            self.messages
                                .compact_task_messages(task_start_offset, error.clone());
                            return error;
                        }
                    };

                    for tool_call in tool_calls {
                        let progress_msg = ChannelResponse {
                            task_id,
                            payload: format_tool_call_progress(
                                &tool_call.function.name,
                                &tool_call.function.arguments,
                                self.debug_mode,
                            ),
                            status: ResponseStatus::Continue,
                        };
                        if let Err(e) = self.channel_tx.send(progress_msg).await {
                            log::error!("Failed to send updates: {}", e);
                        }

                        log::info!("Calling tool: {}", tool_call.function.name);
                        let tool_result = tool_call
                            .execute(
                                task_id,
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
                    let content = match assistant_message.content.clone() {
                        Some(content) => content,
                        None => {
                            let error = "Error: Empty message".to_string();
                            self.messages
                                .compact_task_messages(task_start_offset, error.clone());
                            return error;
                        }
                    };

                    self.messages
                        .compact_task_messages(task_start_offset, content.clone());

                    if self.debug_mode {
                        let debug_info = json!({
                            "timings": response.timings,
                            "usage": response.usage
                        });
                        let formatted =
                            serde_json::to_string_pretty(&debug_info).unwrap_or_else(|_e| {
                                format!("{{\"error\":\"Failed to format debug info\"}}")
                            });
                        return format!("{}\n\n{}", content, formatted);
                    }

                    return content;
                }
            };
        }
    }
}

fn format_tool_call_progress(name: &str, arguments: &Value, debug_mode: bool) -> String {
    if debug_mode {
        format!(
            "Calling tool `{}`\n```json\n{}\n```",
            name,
            serde_json::to_string_pretty(arguments)
                .unwrap_or_else(|_| arguments.to_string())
        )
    } else {
        format!("Calling tool `{}`", name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    impl Agent {
        pub(crate) fn new_for_tests(system_prompt: &str, context_size: u32) -> Self {
            let (task_tx, _task_rx) = tokio::sync::mpsc::channel(1);
            let (channel_tx, _channel_rx) = tokio::sync::mpsc::channel(1);

            Self {
                api_url: reqwest::Url::parse("http://localhost/v1/chat/completions")
                    .expect("test API URL should parse"),
                api_key: "test-key".to_string(),
                model: "test-model".to_string(),
                context_size,
                debug_mode: false,
                environment: std::collections::HashMap::new(),
                skill_store: crate::agent::skills::SkillStore::default(),
                tools: vec![],
                client: reqwest::Client::builder()
                    .no_proxy()
                    .build()
                    .expect("test client should build"),
                messages: MessageHistory::new(vec![Message::new(
                    Role::System,
                    system_prompt.to_string(),
                )]),
                task_manager: std::sync::Arc::new(crate::tasks::TaskManager::new(
                    task_tx,
                    std::sync::Arc::new(crate::tasks::TaskRouter::default()),
                )),
                memory_store: crate::agent::MemoryStore::new(":memory:")
                    .expect("test memory store should initialize"),
                embedding: crate::agent::Embedding {
                    api_url: reqwest::Url::parse("http://localhost/v1")
                        .expect("test embedding URL should parse"),
                    api_key: "test-key".to_string(),
                    model: "test-embedding-model".to_string(),
                    client: reqwest::Client::builder()
                        .no_proxy()
                        .build()
                        .expect("test embedding client should build"),
                },
                channel_tx,
            }
        }
    }

    #[test]
    fn compact_task_messages_drops_tool_intermediates() {
        let mut history = MessageHistory::new(vec![
            Message::new(Role::System, "system".to_string()),
            Message::new(Role::User, "previous user".to_string()),
            Message::new(Role::Assistant, "previous assistant".to_string()),
        ]);
        let task_start_offset = history.len();

        history.push(Message::new(Role::User, "new task".to_string()));
        history.push(Message {
            role: Role::Assistant,
            content: None,
            reasoning_content: None,
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![]),
        });
        history.push(
            Message::new(Role::Tool, "tool output".to_string())
                .with_name("shell".to_string())
                .with_tool_call_id("call_1".to_string()),
        );
        history.push(Message::new(Role::Assistant, "final assistant".to_string()));

        history.compact_task_messages(task_start_offset, "final assistant".to_string());

        assert_eq!(history.len(), 5);
        assert_eq!(history[0].role, Role::System);
        assert_eq!(history[1].role, Role::User);
        assert_eq!(history[2].role, Role::Assistant);
        assert_eq!(history[3].role, Role::User);
        assert_eq!(history[3].content.as_deref(), Some("new task"));
        assert_eq!(history[4].role, Role::Assistant);
        assert_eq!(history[4].content.as_deref(), Some("final assistant"));
    }

    #[test]
    fn compact_task_messages_keeps_no_tool_exchange() {
        let mut history =
            MessageHistory::new(vec![Message::new(Role::System, "system".to_string())]);
        let task_start_offset = history.len();

        history.push(Message::new(Role::User, "question".to_string()));
        history.push(Message::new(Role::Assistant, "answer".to_string()));

        history.compact_task_messages(task_start_offset, "answer".to_string());

        assert_eq!(history.len(), 3);
        assert_eq!(history[1].role, Role::User);
        assert_eq!(history[1].content.as_deref(), Some("question"));
        assert_eq!(history[2].role, Role::Assistant);
        assert_eq!(history[2].content.as_deref(), Some("answer"));
    }

    #[test]
    fn compact_task_messages_preserves_error_exchange_only() {
        let mut history =
            MessageHistory::new(vec![Message::new(Role::System, "system".to_string())]);
        let task_start_offset = history.len();

        history.push(Message::new(Role::User, "broken task".to_string()));
        history.push(Message {
            role: Role::Assistant,
            content: None,
            reasoning_content: None,
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![]),
        });
        history.push(Message::new(Role::Tool, "partial output".to_string()));

        history.compact_task_messages(task_start_offset, "Error: request failed".to_string());

        assert_eq!(history.len(), 3);
        assert_eq!(history[1].role, Role::User);
        assert_eq!(history[1].content.as_deref(), Some("broken task"));
        assert_eq!(history[2].role, Role::Assistant);
        assert_eq!(history[2].content.as_deref(), Some("Error: request failed"));
    }

    #[test]
    fn compact_task_messages_keeps_long_lived_history_visible_across_runs() {
        let mut history =
            MessageHistory::new(vec![Message::new(Role::System, "system".to_string())]);

        let first_task_offset = history.len();
        history.push(Message::new(Role::User, "first task".to_string()));
        history.push(Message::new(Role::Assistant, "first answer".to_string()));
        history.compact_task_messages(first_task_offset, "first answer".to_string());

        let second_task_offset = history.len();
        history.push(Message::new(Role::User, "second task".to_string()));
        history.push(Message {
            role: Role::Assistant,
            content: None,
            reasoning_content: None,
            name: None,
            tool_call_id: None,
            tool_calls: Some(vec![]),
        });
        history.push(Message::new(Role::Tool, "tool output".to_string()));
        history.push(Message::new(Role::Assistant, "second answer".to_string()));
        history.compact_task_messages(second_task_offset, "second answer".to_string());

        assert_eq!(history.len(), 5);
        assert_eq!(history[0].role, Role::System);
        assert_eq!(history[1].content.as_deref(), Some("first task"));
        assert_eq!(history[2].content.as_deref(), Some("first answer"));
        assert_eq!(history[3].content.as_deref(), Some("second task"));
        assert_eq!(history[4].content.as_deref(), Some("second answer"));
        assert!(history.iter().all(|message| message.role != Role::Tool));
    }

    #[test]
    fn clear_preserving_system_removes_conversation_and_resets_tokens() {
        let mut history = MessageHistory::new(vec![
            Message::new(Role::System, "system".to_string()),
            Message::new(Role::User, "question".to_string()),
            Message::new(Role::Assistant, "answer".to_string()),
        ]);
        history.total_tokens = 321;

        history.clear_preserving_system();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, Role::System);
        assert_eq!(history[0].content.as_deref(), Some("system"));
        assert_eq!(history.total_tokens, 0);
    }

    #[test]
    fn clear_preserving_system_is_stable_for_minimal_history() {
        let mut history =
            MessageHistory::new(vec![Message::new(Role::System, "system".to_string())]);
        history.total_tokens = 42;

        history.clear_preserving_system();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, Role::System);
        assert_eq!(history.total_tokens, 0);
    }

    #[test]
    fn tool_progress_message_hides_arguments_in_normal_mode() {
        let payload = format_tool_call_progress("schedule_task", &json!({"delay_seconds": 60}), false);

        assert_eq!(payload, "Calling tool `schedule_task`");
    }

    #[test]
    fn tool_progress_message_includes_arguments_in_debug_mode() {
        let payload = format_tool_call_progress("schedule_task", &json!({"delay_seconds": 60}), true);

        assert!(payload.starts_with("Calling tool `schedule_task`\n```json\n"));
        assert!(payload.contains("\"delay_seconds\": 60"));
        assert!(payload.ends_with("\n```"));
    }
}
