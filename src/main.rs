use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};

use crate::agent::Agent;
use crate::channels::telegram::TelegramChannel;
use crate::channels::Channel;
use crate::tasks::create_task_channel;

mod agent;
mod channels;
mod config;
mod globals;
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
    let (task_manager, task_processor) = create_task_channel().await;
    let task_manager = Arc::new(task_manager);

    // Create MemoryStore and Embedding instances
    let memory_store =
        agent::MemoryStore::new("memory.db").expect("Failed to initialize MemoryStore");
    let embedding =
        agent::Embedding::new(&config.embedding).expect("Failed to initialize Embedding");

    // Create a single Agent instance (shared across all tasks, no cloning)
    let mut agent = Agent::new(&config.agent, task_manager.clone(), memory_store, embedding)
        .expect("Failed to initialize Agent");

    // Set up signal handler for graceful shutdown
    let shutdown_signal = Arc::new(Notify::new());
    let shutdown_clone = shutdown_signal.clone();
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            log::error!("Failed to listen for Ctrl+C: {}", e);
        } else {
            log::info!("Received shutdown signal (Ctrl+C/SIGTERM)");
            shutdown_clone.notify_waiters();
        }
    });

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
                                        match channel.react_with_emoji(&msg.chat_id, msg.message_id, "👍").await {
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
    });

    // Create channel for sending tasks from processor to agent
    let (agent_tx, mut agent_rx) = mpsc::channel::<crate::tasks::Task>(32);

    // Spawn TaskProcessor in a separate coroutine to avoid deadlocks
    tokio::spawn(async move {
        log::info!("TaskProcessor started - waiting for tasks...");
        task_processor.run(agent_tx, shutdown_signal).await;
    });

    // Main loop: Agent runs in main process handling tasks
    // This processes tasks one-by-one, allowing !Send objects to stay safely on this thread
    log::info!("Agent loop started - waiting for tasks...");
    while let Some(task) = agent_rx.recv().await {
        log::info!("Executing task {}", task.id);
        let response = agent.send_message(task.payload).await;
        channel_tx
            .send(response)
            .await
            .expect("Failed to send to channel");
    }

    log::info!("Application shutdown complete");
}
