use reqwest::Url;
use std::env;

use crate::agent::Agent;

mod agent;
mod tools;

/// Main function demonstrating tool integration with an OpenAI-compatible endpoint.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let api_url = env::var("OPENAI_API_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".to_string());
    let api_url = Url::parse(&api_url).expect("API URL is invalid!");
    let api_key = env::var("OPENAI_API_KEY").unwrap_or_else(|_| "ollama".to_string());
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "llama3.1".to_string());

    let mut agent = Agent::new(api_url, api_key, model, tools::get_all_tools());
    let prompt = "What time is it now?".to_string();

    let response = agent.send_message(prompt).await;

    log::info!("{}", response);
}
