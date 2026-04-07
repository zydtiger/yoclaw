use super::BotCommand;
use chrono::{DateTime, Utc};

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
            BotCommand {
                command: "tasks".to_string(),
                description: "List pending tasks for this chat".to_string(),
            },
        ];

        Self { commands }
    }

    /// Takes a command string (e.g. "/start") and returns the appropriate response,
    /// or None if the command is unrecognized.
    pub async fn execute(
        &self,
        cmd: &str,
        current_task_id: crate::tasks::TaskId,
        agent: &mut crate::agent::Agent,
        task_manager: &crate::tasks::TaskManager,
        task_router: &crate::tasks::TaskRouter,
    ) -> Option<String> {
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
            "/tasks" => {
                let Some(chat_id) = task_router.get(&current_task_id).await else {
                    log::error!(
                        "Failed to resolve chat_id for /tasks command task {}",
                        current_task_id
                    );
                    return Some("Unable to determine the current chat.".to_string());
                };

                let all_tasks = task_manager.list_tasks().await;
                let mut rows = Vec::new();

                for task in all_tasks {
                    if task_router.get(&task.id).await.as_deref() == Some(chat_id.as_str()) {
                        rows.push(format!(
                            "- **Delay:** {}, **Payload:** {}",
                            format_delay(task.deadline),
                            escape_markdown_table_cell(&task.payload)
                        ));
                    }
                }

                if rows.is_empty() {
                    "No pending tasks for this chat.".to_string()
                } else {
                    format!("Pending tasks for this chat:\n{}", rows.join("\n"))
                }
            }
            _ => return None,
        };

        Some(response_text)
    }
}

fn format_delay(deadline: DateTime<Utc>) -> String {
    let remaining = (deadline - Utc::now())
        .to_std()
        .unwrap_or(std::time::Duration::ZERO);

    let total_seconds = remaining.as_secs();
    let days = total_seconds / 86_400;
    let hours = (total_seconds % 86_400) / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }

    parts.join(" ")
}

fn escape_markdown_table_cell(input: &str) -> String {
    input.replace('\n', " ").replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::{Duration, Utc};
    use tokio::sync::mpsc;

    use crate::agent::{Agent, Message, Role};
    use crate::tasks::{Task, TaskCommand, TaskManager, TaskRouter};

    #[tokio::test]
    async fn clear_command_resets_history_and_context_usage() {
        let manager = super::super::CommandManager::new();
        let mut agent = Agent::new_for_tests("test system prompt", 1024);
        let system_message = agent.messages[0].content.clone();
        let (task_tx, _task_rx) = mpsc::channel(1);
        let task_router = Arc::new(TaskRouter::default());
        let task_manager = TaskManager::new(task_tx, task_router.clone());
        let current_task_id = uuid::Uuid::now_v7();
        task_router
            .insert(current_task_id, "chat-1".to_string())
            .await;

        agent
            .messages
            .push(Message::new(Role::User, "question".to_string()));
        agent
            .messages
            .push(Message::new(Role::Assistant, "answer".to_string()));
        agent.messages.total_tokens = 256;

        let clear_response = manager
            .execute(
                "/clear",
                current_task_id,
                &mut agent,
                &task_manager,
                task_router.as_ref(),
            )
            .await;
        let context_response = manager
            .execute(
                "/context",
                current_task_id,
                &mut agent,
                &task_manager,
                task_router.as_ref(),
            )
            .await;

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

    #[tokio::test]
    async fn help_command_includes_clear_and_tasks() {
        let manager = super::super::CommandManager::new();
        let mut agent = Agent::new_for_tests("test system prompt", 1024);
        let (task_tx, _task_rx) = mpsc::channel(1);
        let task_router = Arc::new(TaskRouter::default());
        let task_manager = TaskManager::new(task_tx, task_router.clone());
        let current_task_id = uuid::Uuid::now_v7();
        task_router
            .insert(current_task_id, "chat-1".to_string())
            .await;

        let help_response = manager
            .execute(
                "/help",
                current_task_id,
                &mut agent,
                &task_manager,
                task_router.as_ref(),
            )
            .await
            .expect("help command should exist");

        assert!(help_response.contains("/clear - Clear agent message history"));
        assert!(help_response.contains("/tasks - List pending tasks for this chat"));
    }

    #[tokio::test]
    async fn tasks_command_lists_only_current_chat_tasks() {
        let manager = super::super::CommandManager::new();
        let mut agent = Agent::new_for_tests("test system prompt", 1024);
        let (task_tx, mut task_rx) = mpsc::channel(1);
        let task_router = Arc::new(TaskRouter::default());
        let task_manager = TaskManager::new(task_tx, task_router.clone());
        let current_task_id = uuid::Uuid::now_v7();
        let same_chat_task_id = uuid::Uuid::now_v7();
        let other_chat_task_id = uuid::Uuid::now_v7();

        task_router
            .insert(current_task_id, "chat-1".to_string())
            .await;
        task_router
            .insert(same_chat_task_id, "chat-1".to_string())
            .await;
        task_router
            .insert(other_chat_task_id, "chat-2".to_string())
            .await;

        let same_chat_task = Task {
            id: same_chat_task_id,
            payload: "remind me".to_string(),
            deadline: Utc::now() + Duration::minutes(5),
            repeat: None,
        };
        let other_chat_task = Task {
            id: other_chat_task_id,
            payload: "other chat task".to_string(),
            deadline: Utc::now() + Duration::minutes(10),
            repeat: None,
        };

        let responder = tokio::spawn(async move {
            match task_rx.recv().await {
                Some(TaskCommand::ListTasks(reply_tx)) => {
                    reply_tx
                        .send(vec![same_chat_task, other_chat_task])
                        .expect("list task response should be sent");
                }
                _ => panic!("expected ListTasks command"),
            }
        });

        let tasks_response = manager
            .execute(
                "/tasks",
                current_task_id,
                &mut agent,
                &task_manager,
                task_router.as_ref(),
            )
            .await
            .expect("/tasks command should exist");

        responder.await.expect("list task responder should finish");

        assert!(tasks_response.contains("Pending tasks for this chat:"));
        assert!(tasks_response.contains("- **Delay:** "));
        assert!(tasks_response.contains("**Payload:** remind me"));
        assert!(!tasks_response.contains("other chat task"));
    }

    #[tokio::test]
    async fn tasks_command_reports_empty_chat_queue() {
        let manager = super::super::CommandManager::new();
        let mut agent = Agent::new_for_tests("test system prompt", 1024);
        let (task_tx, mut task_rx) = mpsc::channel(1);
        let task_router = Arc::new(TaskRouter::default());
        let task_manager = TaskManager::new(task_tx, task_router.clone());
        let current_task_id = uuid::Uuid::now_v7();

        task_router
            .insert(current_task_id, "chat-1".to_string())
            .await;

        let responder = tokio::spawn(async move {
            match task_rx.recv().await {
                Some(TaskCommand::ListTasks(reply_tx)) => {
                    reply_tx
                        .send(Vec::new())
                        .expect("empty list response should be sent");
                }
                _ => panic!("expected ListTasks command"),
            }
        });

        let tasks_response = manager
            .execute(
                "/tasks",
                current_task_id,
                &mut agent,
                &task_manager,
                task_router.as_ref(),
            )
            .await;

        responder.await.expect("list task responder should finish");

        assert_eq!(
            tasks_response.as_deref(),
            Some("No pending tasks for this chat.")
        );
    }
}
