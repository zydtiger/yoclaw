use clap::Parser;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use crate::agent::Agent;
use crate::channels::telegram::TelegramChannel;
use crate::channels::{Channel, ChannelHandler, ChannelResponse, ResponseStatus};
use crate::cli::{Cli, Commands, SkillCommands};
use crate::tasks::create_task_channel;

mod agent;
mod channels;
mod cli;
mod config;
mod globals;
mod tasks;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let result = match cli.command {
        Some(Commands::Skill {
            command: SkillCommands::Add { source },
        }) => run_skill_add(source).await,
        None => run_runtime().await,
    };

    if let Err(error) = result {
        eprintln!("Error: {error}");
        std::process::exit(1);
    }
}

async fn run_skill_add(source: crate::cli::SkillSource) -> Result<(), Box<dyn std::error::Error>> {
    let installed = crate::agent::skills::install::install_skill(&source).await?;
    println!("Installed skill '{}' at {}", installed.name, installed.path);
    Ok(())
}

async fn run_runtime() -> Result<(), Box<dyn std::error::Error>> {
    let config = config::Config::load().await?;
    let (task_manager, task_processor, task_router) = create_task_channel().await;
    let task_manager = Arc::new(task_manager);

    // Create Telegram channel
    let telegram_token = config.channels.telegram_token.clone();
    let channel = Box::new(TelegramChannel::new(telegram_token));
    let command_manager = crate::channels::CommandManager::new();
    channel
        .register_commands(command_manager.commands.clone())
        .await
        .expect("Failed to register commands");
    let channel_handler = ChannelHandler::new(
        channel,
        config.channels.allowed_users.clone(),
        config.channels.recv_confirm.clone(),
        task_router,
    );
    let (channel_tx, channel_rx) = mpsc::channel::<ChannelResponse>(16);

    // Create MemoryStore and Embedding instances
    let memory_store =
        agent::MemoryStore::new("memory.db").expect("Failed to initialize MemoryStore");
    let embedding =
        agent::Embedding::new(&config.embedding).expect("Failed to initialize Embedding");

    // Create a single Agent instance (shared across all tasks, no cloning)
    let mut agent = Agent::new(
        &config.agent,
        config.environment,
        task_manager.clone(),
        memory_store,
        embedding,
        channel_tx.clone(),
    )
    .await
    .expect("Failed to initialize Agent");

    // Set up signal handler for graceful shutdown
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            log::error!("Failed to listen for Ctrl+C: {}", e);
        } else {
            log::info!("Received shutdown signal (Ctrl+C/SIGTERM)");
            let _ = shutdown_tx.send(true);
        }
    });

    // Spawn Telegram listener coroutine for incoming messages
    let handler_shutdown_rx = shutdown_rx.clone();
    let listener_handler = channel_handler.clone();
    let listener_task_manager = task_manager.clone();
    let handler_task = tokio::spawn(async move {
        log::info!("ChannelHandler listener started - waiting for messages...");
        listener_handler
            .start_listening(listener_task_manager, handler_shutdown_rx)
            .await;
    });

    // Spawn Telegram sender coroutine for outgoing responses
    let sender_handler = channel_handler;
    let sender_task = tokio::spawn(async move {
        log::info!("ChannelHandler sender started - waiting for responses...");
        sender_handler.start_sending(channel_rx).await;
    });

    // Create channel for sending tasks from processor to agent
    let (agent_tx, mut agent_rx) = mpsc::channel::<crate::tasks::Task>(32);

    // Spawn TaskProcessor in a separate coroutine to avoid deadlocks
    let processor_shutdown_rx = shutdown_rx.clone();
    let processor_task = tokio::spawn(async move {
        log::info!("TaskProcessor started - waiting for tasks...");
        task_processor.run(agent_tx, processor_shutdown_rx).await;
    });

    // Main loop: Agent runs in main process handling tasks
    // This processes tasks one-by-one, allowing !Send objects to stay safely on this thread
    log::info!("Agent loop started - waiting for tasks...");
    while let Some(task) = agent_rx.recv().await {
        log::info!("Executing task {}", task.id);

        let response = if task.payload.starts_with('/') {
            let parts: Vec<&str> = task.payload.splitn(2, ' ').collect();
            let cmd = parts[0];
            log::info!("Received command: {}", cmd);

            match command_manager.execute(cmd, &mut agent) {
                Some(response_text) => response_text,
                None => "Unknown command. Try /help.".to_string(),
            }
        } else {
            agent.start_task(task.id, task.payload).await
        };

        channel_tx
            .send(ChannelResponse {
                task_id: task.id,
                payload: response,
                status: ResponseStatus::Terminate,
            })
            .await
            .unwrap_or_else(|e| log::error!("Failed to send final response to channel: {}", e));
    }

    let _ = handler_task.await;
    let _ = processor_task.await;

    drop(agent);
    drop(channel_tx);

    let _ = sender_task.await;

    log::info!("Application shutdown complete");
    Ok(())
}
