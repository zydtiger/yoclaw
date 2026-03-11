use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod builtins;

/// Represents a tool definition for use with the API.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tool {
    r#type: String,
    function: FunctionTool,
}

/// Represents a function tool with its name, description, and parameters.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct FunctionTool {
    name: String,
    description: String,
    parameters: Parameters,
}

/// Represents the parameters schema for a function tool.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Parameters {
    r#type: String,
    properties: Value,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    required: Option<Vec<String>>,
}

/// Represents a function tool call with its name and arguments.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FunctionToolCall {
    pub name: String,
    pub arguments: Value,
}

/// Represents a tool call with its kind, id, and function call.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    pub r#type: String,
    pub id: String,
    pub function: FunctionToolCall,
}

impl ToolCall {
    pub async fn execute(
        &self,
        task_manager: std::sync::Arc<crate::tasks::task_manager::TaskManager>,
        embedding: &crate::agent::Embedding,
        memory_store: &crate::agent::MemoryStore,
    ) -> String {
        match self.function.name.as_str() {
            "get_current_time" => builtins::get_current_time(self.function.arguments.clone()),
            "generic_shell" => builtins::generic_shell(self.function.arguments.clone()).await,
            "read_file" => builtins::read_file(self.function.arguments.clone()).await,
            "write_file" => builtins::write_file(self.function.arguments.clone()).await,
            "get_url" => builtins::get_url(self.function.arguments.clone()).await,
            "schedule_task" => {
                builtins::schedule_task(self.function.arguments.clone(), task_manager).await
            }
            "cancel_task" => {
                builtins::cancel_task(self.function.arguments.clone(), task_manager).await
            }
            "list_tasks" => {
                builtins::list_tasks(self.function.arguments.clone(), task_manager).await
            }
            "add_memory" => {
                builtins::add_memory(self.function.arguments.clone(), embedding, memory_store).await
            }
            "remove_memory" => {
                builtins::remove_memory(self.function.arguments.clone(), memory_store).await
            }
            "search_memory" => {
                builtins::search_memory(self.function.arguments.clone(), embedding, memory_store).await
            }
            _ => format!("Error: Unknown tool '{}'", self.function.name),
        }
    }
}

/// Returns a list of all available tools.
pub fn get_all_tools() -> Vec<Tool> {
    vec![
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "get_current_time".to_string(),
                description: "Returns the current date and time".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({}),
                    required: None,
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "generic_shell".to_string(),
                description: "Executes a shell command and returns the output".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "command": {
                            "type": "string",
                            "description": "The shell command to execute".to_string()
                        }
                    }),
                    required: Some(vec!["command".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "read_file".to_string(),
                description: "Reads the contents of a file".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "path": {
                            "type": "string",
                            "description": "The path to the file to read".to_string()
                        }
                    }),
                    required: Some(vec!["path".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "write_file".to_string(),
                description: "Writes content to a file".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "path": {
                            "type": "string",
                            "description": "The path to the file to write".to_string()
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file".to_string()
                        }
                    }),
                    required: Some(vec!["path".to_string(), "content".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "get_url".to_string(),
                description: "Fetches content from a URL".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "url": {
                            "type": "string",
                            "description": "The URL to fetch content from".to_string()
                        }
                    }),
                    required: Some(vec!["url".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "schedule_task".to_string(),
                description: "Schedules a message payload to be sent to the agent itself after a specified delay in seconds. Use this to remind yourself to do things in the future.".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "payload": {
                            "type": "string",
                            "description": "The message payload to send to the agent".to_string()
                        },
                        "delay_seconds": {
                            "type": "number",
                            "description": "The delay in seconds before the task is executed".to_string()
                        }
                    }),
                    required: Some(vec!["payload".to_string(), "delay_seconds".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "cancel_task".to_string(),
                description: "Cancels a pending scheduled task by its unique ID.".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "task_id": {
                            "type": "number",
                            "description": "The unique ID of the task to cancel".to_string()
                        }
                    }),
                    required: Some(vec!["task_id".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "list_tasks".to_string(),
                description: "Lists all currently scheduled and pending tasks along with their IDs, payloads, and deadlines".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({}),
                    required: None,
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "add_memory".to_string(),
                description: "Adds a new memory string into the vector database".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "text": {
                            "type": "string",
                            "description": "The textual memory to store".to_string()
                        }
                    }),
                    required: Some(vec!["text".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "remove_memory".to_string(),
                description: "Removes a memory from the vector database by ID".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "id": {
                            "type": "number",
                            "description": "The ID of the memory to remove".to_string()
                        }
                    }),
                    required: Some(vec!["id".to_string()]),
                },
            },
        },
        Tool {
            r#type: "function".to_string(),
            function: FunctionTool {
                name: "search_memory".to_string(),
                description: "Searches for relevant memories using semantic similarity".to_string(),
                parameters: Parameters {
                    r#type: "object".to_string(),
                    properties: json!({
                        "query": {
                            "type": "string",
                            "description": "The search query".to_string()
                        },
                        "top_k": {
                            "type": "number",
                            "description": "The maximum number of results to return".to_string()
                        }
                    }),
                    required: Some(vec!["query".to_string(), "top_k".to_string()]),
                },
            },
        },
    ]
}
