use std::{sync::Arc, time::Duration};

use crate::tasks::TaskManager;

use super::{Channel, ChannelHandler};

impl ChannelHandler {
    pub fn new(channel: Box<dyn Channel>) -> Self {
        Self { channel }
    }

    pub async fn start_listening(
        &self,
        mut channel_rx: tokio::sync::mpsc::Receiver<String>,
        task_manager: Arc<TaskManager>,
    ) {
        let chat_id = "7235677031"; // TODO: hard-code chat_id for now

        loop {
            tokio::select! {
                // Branch 1: Send outgoing messages
                Some(msg) = channel_rx.recv() => {
                    if let Err(e) = self.channel.send_message(chat_id, &msg).await {
                        log::error!("Failed to send message to Telegram: {}", e);
                    }
                }

                // Branch 2: Poll incoming messages
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

                                // Schedule the incoming message as a task for the agent to process
                                match task_manager.schedule_task(msg.text).await {
                                    Ok(task_id) => {
                                        log::info!("Scheduled task #{} for incoming message", task_id);
                                        // TODO: make response configurable
                                        match self.channel.react_with_emoji(&msg.chat_id, msg.message_id, "👍").await {
                                            Ok(()) => log::info!("Successfully reacted to user message"),
                                            Err(e) => log::error!("Failed to respond: {}", e),
                                        };
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
            }
        }
    }
}
