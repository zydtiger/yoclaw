use reqwest::Client;
use serde_json::{json, Value};
use std::env;

use crate::agent::{Message, Role};

mod agent;
mod tools;

/// Call an OpenAI-compatible API with tool support.
async fn call_model_with_tools(
    client: &Client,
    messages: &[Message],
    tools: &[tools::Tool],
) -> Result<Value, String> {
    let api_url = env::var("OPENAI_API_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1/chat/completions".to_string());
    let api_key = env::var("OPENAI_API_KEY").unwrap_or_else(|_| "ollama".to_string());
    let model = env::var("OPENAI_MODEL").unwrap_or_else(|_| "llama3.1".to_string());

    let payload = json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "tool_choice": "auto"
    });

    let response = client
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&payload)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let result = response.json::<Value>().await.map_err(|e| e.to_string())?;

    Ok(result)
}

/// Main function demonstrating tool integration with an OpenAI-compatible endpoint.
#[tokio::main]
async fn main() {
    let client = Client::new();
    let tools = tools::get_all_tools();
    let mut messages = vec![Message::new(Role::User, "What time is it now?".to_string())];

    println!("=== Tool Integration Demo ===");
    println!("Calling model with message: {}", &messages[0].content);
    println!();

    println!("Step 1: Calling model...");
    let response = match call_model_with_tools(&client, &messages, &tools).await {
        Ok(res) => res,
        Err(e) => {
            println!("Error: {}", e);
            return;
        }
    };

    if let Some(error) = response.get("error") {
        println!("Error: {:?}", error);
        println!("\nMake sure you have:");
        println!("1. An OpenAI-compatible API running (e.g., Ollama at http://localhost:11434)");
        println!("2. Set OPENAI_API_URL environment variable if using a different endpoint");
        println!("3. Set OPENAI_MODEL to the model you want to use");
        return;
    }

    if let Some(choices) = response.get("choices").and_then(|c| c.as_array()) {
        if !choices.is_empty() {
            let choice = &choices[0];
            let assistant_message = choice.get("message").cloned().unwrap_or(json!({}));
            let assistant_message: Message = serde_json::from_value(assistant_message)
                .expect("Failed to deserialize assistant message");

            println!(
                "Model response: {}",
                serde_json::to_string_pretty(&assistant_message).unwrap_or_default()
            );
            println!();

            let tool_calls = &assistant_message.tool_calls;

            if let Some(calls) = tool_calls {
                if !calls.is_empty() {
                    println!("Step 2: Model wants to call a tool!");

                    // Append the complete assistant message (including tool_calls) to the history
                    messages.push(assistant_message.clone());

                    for tool_call in calls {
                        println!("Calling tool: {}", &tool_call.function.name);
                        let tool_result = tool_call.execute();
                        println!("Tool result: {}", tool_result);
                        println!();

                        let message = Message::new(Role::Tool, tool_result)
                            .with_name(tool_call.function.name.clone())
                            .with_tool_call_id(tool_call.id.clone());
                        messages.push(message);
                    }

                    println!("Step 3: Calling model with tool results...");
                    let final_response = call_model_with_tools(&client, &messages, &tools)
                        .await
                        .unwrap_or(json!({}));

                    if let Some(final_choices) =
                        final_response.get("choices").and_then(|c| c.as_array())
                    {
                        if !final_choices.is_empty() {
                            let final_msg = final_choices[0]
                                .get("message")
                                .and_then(|m| m.get("content"))
                                .and_then(|c| c.as_str())
                                .unwrap_or("");
                            println!("Final response: {}", final_msg);
                        }
                    }
                } else {
                    println!("Model response: {}", &assistant_message.content);
                }
            } else {
                println!("Model response: {}", &assistant_message.content);
            }
        }
    }
}
