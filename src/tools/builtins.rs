use chrono::Local;
use serde_json::Value;

/// Returns the current date and time as a formatted string.
pub fn get_current_time(_args: Value) -> String {
    Local::now().format("%Y-%m-%d %A %H:%M:%S").to_string()
}

/// Executes generic shell command and return command output.
/// Expects args to be { "command": string }
pub async fn generic_shell(args: Value) -> String {
    let args = if let Some(inner_str) = args.as_str() {
        // If it's a string, we MUST be able to decode it.
        match serde_json::from_str::<Value>(inner_str) {
            Ok(v) => v,
            Err(e) => return format!("Error: Failed to decode inner JSON: {}", e),
        }
    } else {
        // If it's already an object, use it directly
        args
    };

    let command = match args.pointer("/command").and_then(|v| v.as_str()) {
        Some(cmd) => cmd.to_string(),
        None => return "Error: 'command' field missing or not a string".to_string(),
    };
    log::info!("Executing command: {}", command);

    // Split the command into program and arguments
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return "Error: empty command".to_string();
    }

    let program = parts[0];
    let cmd_args = &parts[1..];

    // Execute the program directly with its arguments
    let output = match tokio::process::Command::new(program)
        .args(cmd_args)
        .output()
        .await
    {
        Ok(output) => output,
        Err(e) => return format!("Error executing command: {}", e),
    };

    // Build the result from stdout and stderr
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        if stdout.is_empty() && stderr.is_empty() {
            return "Command executed successfully (no output)".to_string();
        }
        let mut result = stdout.trim().to_string();
        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push_str("\n");
            }
            result.push_str(&format!("(stderr: {})", stderr.trim()));
        }
        result
    } else {
        format!(
            "Command failed with exit code: {}\n{}",
            output.status,
            stderr.trim()
        )
    }
}

/// Reads file contents.
/// Expects args to be { "path": string }
pub async fn read_file(args: Value) -> String {
    let args = if let Some(inner_str) = args.as_str() {
        // If it's a string, we MUST be able to decode it.
        match serde_json::from_str::<Value>(inner_str) {
            Ok(v) => v,
            Err(e) => return format!("Error: Failed to decode inner JSON: {}", e),
        }
    } else {
        // If it's already an object, use it directly
        args
    };

    let path = match args.pointer("/path").and_then(|v| v.as_str()) {
        Some(cmd) => cmd.to_string(),
        None => return "Error: 'path' field missing or not a string".to_string(),
    };
    log::info!("Reading file: {}", path);

    // Read the file asynchronously using tokio
    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => contents,
        Err(e) => format!("Error reading file '{}': {}", path, e),
    }
}

/// Writes content to a file.
/// Expects args to be { "path": string, "content": string }
pub async fn write_file(args: Value) -> String {
    let args = if let Some(inner_str) = args.as_str() {
        // If it's a string, we MUST be able to decode it.
        match serde_json::from_str::<Value>(inner_str) {
            Ok(v) => v,
            Err(e) => return format!("Error: Failed to decode inner JSON: {}", e),
        }
    } else {
        // If it's already an object, use it directly
        args
    };

    let path = match args.pointer("/path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return "Error: 'path' field missing or not a string".to_string(),
    };
    log::info!("Writing file: {}", path);

    let content = match args.pointer("/content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return "Error: 'content' field missing or not a string".to_string(),
    };

    // Write the file asynchronously using tokio
    match tokio::fs::write(&path, &content).await {
        Ok(_) => format!("Successfully wrote {} bytes to '{}'", content.len(), path),
        Err(e) => format!("Error writing file '{}': {}", path, e),
    }
}

/// Fetches content from a URL and returns the response.
/// Expects args to be { "url": string }
pub async fn get_url(args: Value) -> String {
    let args = if let Some(inner_str) = args.as_str() {
        // If it's a string, we MUST be able to decode it.
        match serde_json::from_str::<Value>(inner_str) {
            Ok(v) => v,
            Err(e) => return format!("Error: Failed to decode inner JSON: {}", e),
        }
    } else {
        // If it's already an object, use it directly
        args
    };

    let url = match args.pointer("/url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => return "Error: 'url' field missing or not a string".to_string(),
    };
    log::info!("Fetching URL: {}", url);

    // Fetch the URL using reqwest
    match reqwest::get(&url).await {
        Ok(response) => {
            if !response.status().is_success() {
                return format!("Error: Request failed with status code: {}", response.status());
            }
            match response.text().await {
                Ok(body) => body,
                Err(e) => format!("Error reading response body: {}", e),
            }
        }
        Err(e) => format!("Error fetching URL '{}': {}", url, e),
    }
}
