use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::watch;

use crate::channels::{ChannelResponse, ResponseStatus};
use crate::tasks::{TaskId, TaskManager};

use super::{Channel, ChannelHandler};

impl ChannelHandler {
    pub async fn new(
        channel: Box<dyn Channel>,
        allowed_users: Vec<String>,
        recv_confirm: Option<String>,
    ) -> Self {
        let task_routes = Self::load_routes().await.unwrap_or_else(|e| {
            log::warn!("Failed to load task routes: {}", e);
            HashMap::new()
        });

        Self {
            channel: Arc::from(channel),
            allowed_users,
            recv_confirm,
            task_routes: Arc::new(tokio::sync::Mutex::new(task_routes)),
        }
    }

    /// Load task routes from routes.json
    async fn load_routes() -> Result<HashMap<TaskId, String>, Box<dyn std::error::Error>> {
        let route_path = std::path::PathBuf::from(&*crate::globals::CONFIG_DIR).join("routes.json");
        if !route_path.exists() {
            return Ok(HashMap::new());
        }

        let data = tokio::fs::read_to_string(&route_path).await?;
        let routes: HashMap<TaskId, String> = serde_json::from_str(&data)?;
        log::info!("Loaded {} task route(s) from routes.json", routes.len());
        Ok(routes)
    }

    /// Save task routes to routes.json
    async fn save_routes(&self) -> Result<(), Box<dyn std::error::Error>> {
        let route_path = std::path::PathBuf::from(&*crate::globals::CONFIG_DIR).join("routes.json");
        let task_routes = self.task_routes.lock().await;
        let json = serde_json::to_string_pretty(&*task_routes)?;
        tokio::fs::write(&route_path, json).await?;
        log::info!("Saved {} task route(s) to routes.json", task_routes.len());
        Ok(())
    }

    async fn forward_response(&self, response: ChannelResponse) {
        let task_id = response.task_id;
        let chat_id = {
            let task_routes = self.task_routes.lock().await;
            match task_routes.get(&task_id) {
                Some(id) => id.clone(),
                None => {
                    log::error!(
                        "Failed to route message for task {}: no chat_id found in task_routes. Dropping message.",
                        task_id
                    );
                    return;
                }
            }
        };

        if chat_id.is_empty() {
            log::error!(
                "Failed to route message for task {}: chat_id is empty",
                task_id
            );
            return;
        }

        if let Err(e) = self.channel.send_message(&chat_id, &response.payload).await {
            log::error!("Failed to send message to Telegram: {}", e);
        }

        if response.status == ResponseStatus::Terminate {
            self.task_routes.lock().await.remove(&task_id);
        }
    }

    pub async fn start_listening(
        self,
        task_manager: Arc<TaskManager>,
        mut shutdown_signal: watch::Receiver<bool>,
    ) {
        loop {
            tokio::select! {
                // Branch 1: Poll incoming messages
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    match self.channel.receive_messages().await {
                        Ok(messages) => {
                            for msg in messages {
                                log::info!(
                                    "Received message from {} (chat: {}): {}",
                                    msg.sender_id,
                                    msg.chat_id,
                                    msg.text
                                );
                                let is_unauthorized = !self.allowed_users.is_empty() && !self.allowed_users.contains(&msg.sender_id);
                                let is_empty = self.allowed_users.is_empty();

                                if is_unauthorized || is_empty {
                                    if is_empty {
                                        log::info!("Ignoring message because no users are allowed.");
                                    } else {
                                        log::info!("Ignoring message from unauthorized user: {} ({})", msg.sender_id, msg.sender_name.clone().unwrap_or_default());
                                    }

                                    // Send a warning back to the unauthorized user
                                    let warning_msg = format!("⚠️ You are not allowed to access this bot. Your User ID is {}", msg.sender_id);
                                    if let Err(e) = self.channel.send_message(&msg.chat_id, &warning_msg).await {
                                        log::error!("Failed to send blocked warning message to Telegram: {}", e);
                                    }
                                    continue;
                                }

                                // Schedule the incoming message as a task for the agent to process
                                match task_manager.schedule_task(msg.text).await {
                                    Ok(task_id) => {
                                        self.task_routes
                                            .lock()
                                            .await
                                            .insert(task_id, msg.chat_id.clone());
                                        log::info!("Scheduled task #{} for incoming message", task_id);
                                        if let Some(emoji) = &self.recv_confirm {
                                            match self.channel.react_with_emoji(&msg.chat_id, msg.message_id, emoji).await {
                                                Ok(()) => log::info!("Successfully reacted to user message"),
                                                Err(e) => log::error!("Failed to respond: {}", e),
                                            };
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("Failed to schedule task for incoming message: {}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Error receiving messages from Telegram: {}", e);
                        }
                    }
                }

                // Branch 2: Graceful shutdown
                _ = shutdown_signal.changed() => {
                    if *shutdown_signal.borrow() {
                        log::info!("ChannelHandler listener received shutdown signal, stopping intake...");
                        break;
                    }
                }
            }
        }
    }

    pub async fn start_sending(self, mut channel_rx: tokio::sync::mpsc::Receiver<ChannelResponse>) {
        loop {
            match channel_rx.recv().await {
                Some(response) => self.forward_response(response).await,
                None => break,
            }
        }

        if let Err(e) = self.save_routes().await {
            log::error!("Failed to save routes during sender shutdown: {}", e);
        }
    }
}
