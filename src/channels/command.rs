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
            _ => return None,
        };

        Some(response_text)
    }
}
