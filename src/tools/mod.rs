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
    pub fn execute(&self) -> String {
        match self.function.name.as_str() {
            "get_current_time" => builtins::get_current_time(self.function.arguments.clone()),
            _ => format!("Error: Unknown tool '{}'", self.function.name),
        }
    }
}

/// Returns a list of all available tools.
pub fn get_all_tools() -> Vec<Tool> {
    vec![Tool {
        r#type: "function".to_string(),
        function: FunctionTool {
            name: "get_current_time".to_string(),
            description: "Returns the current date and time".to_string(),
            parameters: Parameters {
                r#type: "object".to_string(),
                properties: json!({}),
            },
        },
    }]
}
