use super::BotCommand;

impl super::CommandManager {
    /// Creates a new CommandManager and registers the default bot commands.
    pub fn new() -> Self {
        let commands = vec![
            BotCommand {
                command: "help".to_string(),
                description: "Show available commands".to_string(),
            },
            BotCommand {
                command: "tools".to_string(),
                description: "List loaded tools".to_string(),
            },
            BotCommand {
                command: "skills".to_string(),
                description: "List loaded skills".to_string(),
            },
            BotCommand {
                command: "context".to_string(),
                description: "Show current context usage".to_string(),
            },
            BotCommand {
                command: "clear".to_string(),
                description: "Clear agent message history".to_string(),
            },
        ];

        Self { commands }
    }

    /// Takes a command string (e.g. "/start") and returns the appropriate response,
    /// or None if the command is unrecognized.
    pub fn execute(&self, cmd: &str, agent: &mut crate::agent::Agent) -> Option<String> {
        let response_text = match cmd {
            "/help" => {
                let mut help_text = String::from("Available commands:\n\n");
                for bot_cmd in &self.commands {
                    help_text
                        .push_str(&format!("/{} - {}\n", bot_cmd.command, bot_cmd.description));
                }
                help_text
            }
            "/tools" => {
                let tool_names: Vec<String> = agent
                    .tools
                    .iter()
                    .map(|t| t.function.name.clone())
                    .collect();
                if tool_names.is_empty() {
                    "No tools loaded.".to_string()
                } else {
                    let numbered_tools: Vec<String> = tool_names
                        .iter()
                        .enumerate()
                        .map(|(i, name)| format!("{}. {}", i + 1, name))
                        .collect();
                    format!("Loaded tools:\n{}", numbered_tools.join("\n"))
                }
            }
            "/skills" => {
                let skill_names: Vec<String> = agent
                    .skill_store
                    .skills
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                if skill_names.is_empty() {
                    "No skills loaded.".to_string()
                } else {
                    let numbered_skills: Vec<String> = skill_names
                        .iter()
                        .enumerate()
                        .map(|(i, name)| format!("{}. {}", i + 1, name))
                        .collect();
                    format!("Loaded skills:\n{}", numbered_skills.join("\n"))
                }
            }
            "/context" => {
                let used_tokens = agent.messages.total_tokens;
                let context_size = agent.context_size;
                let usage_percent = (used_tokens as f64 / context_size as f64) * 100.0;
                format!(
                    "Context usage: {:.1}% ({}/{})",
                    usage_percent, used_tokens, context_size
                )
            }
            "/clear" => {
                agent.clear_history();
                "Agent message history cleared.".to_string()
            }
            _ => return None,
        };

        Some(response_text)
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::{Agent, Message, Role};

    #[test]
    fn clear_command_resets_history_and_context_usage() {
        let manager = super::super::CommandManager::new();
        let mut agent = Agent::new_for_tests("test system prompt", 1024);
        let system_message = agent.messages[0].content.clone();

        agent
            .messages
            .push(Message::new(Role::User, "question".to_string()));
        agent
            .messages
            .push(Message::new(Role::Assistant, "answer".to_string()));
        agent.messages.total_tokens = 256;

        let clear_response = manager.execute("/clear", &mut agent);
        let context_response = manager.execute("/context", &mut agent);

        assert_eq!(
            clear_response.as_deref(),
            Some("Agent message history cleared.")
        );
        assert_eq!(agent.messages.len(), 1);
        assert_eq!(agent.messages[0].role, Role::System);
        assert_eq!(agent.messages[0].content, system_message);
        assert_eq!(agent.messages.total_tokens, 0);
        assert_eq!(
            context_response.as_deref(),
            Some("Context usage: 0.0% (0/1024)")
        );
    }

    #[test]
    fn help_command_includes_clear() {
        let manager = super::super::CommandManager::new();
        let mut agent = Agent::new_for_tests("test system prompt", 1024);

        let help_response = manager
            .execute("/help", &mut agent)
            .expect("help command should exist");

        assert!(help_response.contains("/clear - Clear agent message history"));
    }
}
