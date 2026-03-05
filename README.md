# Tool Integration with OpenAI-Compatible Endpoint

A minimal implementation of tool calling with an OpenAI-compatible API endpoint, implemented in both Python and Rust.

## Features

- Define custom tools that can be called by an LLM
- Execute tool calls and return results to the model
- Support for OpenAI-compatible APIs (e.g., Ollama, vLLM, etc.)
- Dual implementation: Python and Rust

## Project Structure

```
yoclaw/
├── main.py              # Python implementation
├── Cargo.toml           # Rust dependencies
└── src/
    └── main.rs          # Rust implementation
```

## Setup

### Prerequisites

1. An OpenAI-compatible API endpoint (e.g., [Ollama](https://ollama.com/))
2. A model that supports tool calling (e.g., `llama3.1`, `llama3.2`)

### Install Ollama (if not using existing endpoint)

```bash
# Install Ollama
curl -fsSL https://ollama.com/install.sh | sh

# Pull a model that supports tool calling
ollama pull llama3.1

# Start the Ollama server
ollama serve
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OPENAI_API_URL` | `http://localhost:11434/v1/chat/completions` | API endpoint |
| `OPENAI_API_KEY` | `ollama` | API key (not required for local Ollama) |
| `OPENAI_MODEL` | `llama3.1` | Model name to use |

## Usage

### Python

```bash
# Run with default settings (requires Ollama running locally)
python main.py

# Run with custom endpoint
OPENAI_API_URL="https://api.example.com/v1/chat/completions" \
OPENAI_MODEL="your-model" \
python main.py
```

### Rust

```bash
# Build and run
cargo run

# Or with custom endpoint
OPENAI_API_URL="https://api.example.com/v1/chat/completions" \
OPENAI_MODEL="your-model" \
cargo run
```

## How It Works

1. **Define Tools**: Create tool definitions in the `TOOLS` list
2. **Implement Tools**: Add tool implementations to `TOOL_IMPLEMENTATIONS`
3. **Call Model**: Send messages with available tools to the API
4. **Execute Tools**: If model wants to call a tool, execute it
5. **Return Results**: Send tool results back to model for final response

## Example Flow

```
User: "What time is it now?"
  ↓
Model: [tool_call: get_current_time()]
  ↓
Tool: Returns "2025-03-05 15:30:00"
  ↓
Model: "The current time is 2025-03-05 15:30:00."
```

## Adding New Tools (Python)

```python
# 1. Define the tool function
def weather_forecast(city: str) -> str:
    """Returns the weather forecast for a city."""
    return f"Sunny in {city}, 75°F"

# 2. Add tool definition
TOOLS.append({
    "type": "function",
    "function": {
        "name": "weather_forecast",
        "description": "Returns the weather forecast",
        "parameters": {
            "type": "object",
            "properties": {
                "city": {"type": "string", "description": "City name"}
            },
            "required": ["city"]
        }
    }
})

# 3. Register implementation
TOOL_IMPLEMENTATIONS["weather_forecast"] = weather_forecast
```

## Adding New Tools (Rust)

```rust
// 1. Define the tool function
fn weather_forecast(city: &str) -> String {
    format!("Sunny in {}, 75°F", city)
}

// 2. Add to get_all_tools()
fn get_all_tools() -> Vec<Value> {
    vec![
        json!({...}), // existing tools
        json!({
            "type": "function",
            "function": {
                "name": "weather_forecast",
                "description": "Returns the weather forecast",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string", "description": "City name"}
                    },
                    "required": ["city"]
                }
            }
        })
    ]
}

// 3. Add to execute_tool_call match
async fn execute_tool_call(tool_name: &str, tool_args: &Value) -> Result<String, String> {
    match tool_name {
        "get_current_time" => Ok(get_current_time()),
        "weather_forecast" => {
            let city = tool_args.get("city").and_then(|c| c.as_str()).unwrap_or("unknown");
            Ok(weather_forecast(city))
        }
        _ => Err(format!("Unknown tool: {}", tool_name)),
    }
}