use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

use crate::channels::{ChannelResponse, ResponseStatus};
use crate::tasks::{TaskManager, TaskRouteBinding, TaskRouter};

use super::{Channel, ChannelHandler};

impl ChannelHandler {
    pub fn new(
        channel: Box<dyn Channel>,
        allowed_users: Vec<String>,
        recv_confirm: Option<String>,
        task_router: Arc<TaskRouter>,
    ) -> Self {
        Self {
            channel: Arc::from(channel),
            allowed_users,
            recv_confirm,
            task_router,
        }
    }

    async fn forward_response(&self, response: ChannelResponse) {
        let task_id = response.task_id;
        let chat_id = match self.task_router.get(&task_id).await {
            Some(id) => id,
            None => {
                log::error!(
                    "Failed to route message for task {}: no chat_id found in task_routes. Dropping message.",
                    task_id
                );
                return;
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
            self.task_router.remove(&task_id).await;
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
                                match task_manager.schedule_task(
                                    msg.text,
                                    None,
                                    None,
                                    TaskRouteBinding::ChatId(msg.chat_id.clone()),
                                ).await {
                                    Ok(task_id) => {
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

        if let Err(e) = self.task_router.save().await {
            log::error!("Failed to save routes during sender shutdown: {}", e);
        }
    }
}
