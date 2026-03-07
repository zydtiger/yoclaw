use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::agent::Agent;
use crate::channels::telegram::TelegramChannel;
use crate::channels::Channel;
use crate::tasks::task_manager::create_task_channel;

mod agent;
mod channels;
mod config;
mod tasks;

/// Main function demonstrating tool integration with an OpenAI-compatible endpoint.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let config = config::Config::load().await.expect("Failed to load config");

    // Create Telegram channel
    let telegram_token = config.channels.telegram_token;
    let channel = Arc::new(TelegramChannel::new(telegram_token));

    // Create task channel pair
    let (task_manager, task_processor) = create_task_channel();
    let task_manager = Arc::new(task_manager);

    // Create a single Agent instance (shared across all tasks, no cloning)
    let agent =
        Agent::new(&config.agent, task_manager.clone()).expect("Failed to initialize Agent");

    // Spawn unified Telegram coroutine - handles both polling and sending
    let (channel_tx, mut channel_rx) = mpsc::channel::<String>(16);
    tokio::spawn(async move {
        let chat_id = "7235677031"; // TODO: hard-code chat_id for now

        loop {
            tokio::select! {
                // Branch 1: Send outgoing messages
                Some(msg) = channel_rx.recv() => {
                    if let Err(e) = channel.send_message(chat_id, &msg).await {
                        log::error!("Failed to send message to Telegram: {}", e);
                    }
                }

                // Branch 2: Poll incoming messages
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    match channel.receive_messages().await {
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
    });

    // Main loop: TaskProcessor runs in main process with Agent
    // This processes tasks one-by-one, preserving chat history across all tasks
    log::info!("TaskProcessor started - waiting for tasks...");
    task_processor.run(agent, channel_tx).await;
}
