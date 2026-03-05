use serde_json::{json, Value};

mod builtins;

/// Defines the available tools.
pub fn get_all_tools() -> Vec<Value> {
    vec![json!({
        "type": "function",
        "function": {
            "name": "get_current_time",
            "description": "Returns the current date and time",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        }
    })]
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
