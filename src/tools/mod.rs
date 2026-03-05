use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod builtins;

/// Represents a tool definition for use with the API.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tool {
    #[serde(rename = "type")]
    kind: String,
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
    #[serde(rename = "type")]
    kind: String,
    properties: Value,
}

/// Defines the available tools.
pub fn get_all_tools() -> Vec<Tool> {
    vec![Tool {
        kind: "function".to_string(),
        function: FunctionTool {
            name: "get_current_time".to_string(),
            description: "Returns the current date and time".to_string(),
            parameters: Parameters {
                kind: "object".to_string(),
                properties: json!({}),
            },
        },
    }]
}

/// Execute a tool call and return the result.
pub fn execute_tool(tool_call: &Value) -> String {
    let function = tool_call.get("function");
    let name = function
        .and_then(|f| f.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("");

    // TODO: If you expand this to handle arguments, you would parse them here:
    // let args_str = function.and_then(|f| f.get("arguments")).and_then(|a| a.as_str()).unwrap_or("{}");
    // let _args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

    match name {
        "get_current_time" => builtins::get_current_time(),
        _ => format!("Error: Unknown tool '{}'", name),
    }
}
