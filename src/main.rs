use reqwest::Client;
use serde_json::{json, Value};
use std::env;

mod tools;

/// Call an OpenAI-compatible API with tool support.
async fn call_model_with_tools(
    client: &Client,
    messages: &[Value],
    tools: &[Value],
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
    let mut messages = vec![json!({
        "role": "user",
        "content": "What time is it now?"
    })];

    println!("=== Tool Integration Demo ===");
    println!(
        "Calling model with message: {}",
        messages[0]["content"].as_str().unwrap()
    );
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

            println!(
                "Model response: {}",
                serde_json::to_string_pretty(&assistant_message).unwrap_or_default()
            );
            println!();

            let tool_calls = assistant_message
                .get("tool_calls")
                .and_then(|tc| tc.as_array());

            if let Some(calls) = tool_calls {
                if !calls.is_empty() {
                    println!("Step 2: Model wants to call a tool!");

                    // Append the complete assistant message (including tool_calls) to the history
                    messages.push(assistant_message.clone());

                    for tool_call in calls {
                        let tool_name = tool_call
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown");

                        let tool_id = tool_call.get("id").and_then(|id| id.as_str()).unwrap_or("");

                        println!("Calling tool: {}", tool_name);
                        let tool_result = tools::execute_tool(tool_call);
                        println!("Tool result: {}", tool_result);
                        println!();

                        // Append the tool execution result
                        messages.push(json!({
                            "role": "tool",
                            "content": tool_result,
                            "name": tool_name,
                            "tool_call_id": tool_id
                        }));
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
                    let content = assistant_message
                        .get("content")
                        .and_then(|c| c.as_str())
                        .unwrap_or("");
                    println!("Model response: {}", content);
                }
            } else {
                let content = assistant_message
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                println!("Model response: {}", content);
            }
        }
    }
}
