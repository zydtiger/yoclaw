use chrono::Local;
use serde_json::Value;

/// Returns the current date and time as a formatted string.
pub fn get_current_time(_args: Value) -> String {
    Local::now().format("%Y-%m-%d %A %H:%M:%S").to_string()
}

/// Executes generic shell command and return command output.
/// Expects args to be { "command": string }
pub async fn generic_shell(
    args: Value,
    environment: &std::collections::HashMap<String, String>,
) -> String {

    let command = match args.pointer("/command").and_then(|v| v.as_str()) {
        Some(cmd) => cmd.to_string(),
        None => return "Error: 'command' field missing or not a string".to_string(),
    };

    // Execute the program via sh -c to support complex commands and pipes
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(&command);
    cmd.envs(environment);

    if let Some(cwd) = args.pointer("/cwd").and_then(|v| v.as_str()) {
        log::info!("Setting working directory to: {}", cwd);
        cmd.current_dir(cwd);
    }

    log::info!("Executing command: {}", command);
    let output = match cmd.output().await {
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

/// Retrieves the raw contents of a loaded skill by name dynamically from disk to ensure it's not stale.
pub async fn use_skill(args: Value, skill_store: &crate::agent::skills::SkillStore) -> String {

    let name = match args.pointer("/name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return "Error: 'name' field missing or not a string".to_string(),
    };

    let skill = match skill_store.get_skill(name) {
        Some(s) => s,
        None => return format!("Error: Skill '{}' not found in the skill metadata list.", name),
    };

    // Attempt to load the skill dynamically from disk to get latest changes
    let target_path = std::path::Path::new(&skill.path);

    // If it's a directory, we need to read SKILL.md inside it.
    // If it's a file, we read it directly.
    let file_to_read = if target_path.is_dir() {
        target_path.join("SKILL.md")
    } else {
        target_path.to_path_buf()
    };

    match tokio::fs::read_to_string(&file_to_read).await {
        Ok(contents) => format!(
            "Skill contents for '{}':\n\n{}\n\nPath: {}",
            skill.name, contents, skill.path
        ),
        Err(e) => format!(
            "Error loading skill file from disk at '{}': {}",
            file_to_read.display(),
            e
        ),
    }
}

/// Reads file contents.
/// Expects args to be { "path": string }
pub async fn read_file(args: Value) -> String {

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

    let url = match args.pointer("/url").and_then(|v| v.as_str()) {
        Some(u) => u.to_string(),
        None => return "Error: 'url' field missing or not a string".to_string(),
    };
    log::info!("Fetching URL: {}", url);

    // Fetch the URL using reqwest
    match reqwest::get(&url).await {
        Ok(response) => {
            if !response.status().is_success() {
                return format!(
                    "Error: Request failed with status code: {}",
                    response.status()
                );
            }
            match response.text().await {
                Ok(body) => body,
                Err(e) => format!("Error reading response body: {}", e),
            }
        }
        Err(e) => format!("Error fetching URL '{}': {}", url, e),
    }
}

/// Schedules a new task to be executed after a delay.
/// Expects args to be { "payload": string, "delay_seconds": number }
pub async fn schedule_task(
    args: Value,
    task_manager: std::sync::Arc<crate::tasks::TaskManager>,
) -> String {

    let payload = match args.pointer("/payload").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return "Error: 'payload' field missing or not a string".to_string(),
    };

    let delay_seconds = match args.pointer("/delay_seconds").and_then(|v| v.as_f64()) {
        Some(d) => d as i64,
        None => return "Error: 'delay_seconds' field missing or not a number".to_string(),
    };

    log::info!("Scheduling task in {}s: {}", delay_seconds, payload);

    match task_manager
        .schedule_task_in(payload, chrono::Duration::seconds(delay_seconds))
        .await
    {
        Ok(task_id) => {
            log::info!("Successfully scheduled task #{}", task_id);
            format!("Successfully scheduled task with ID: {}", task_id)
        }
        Err(e) => format!("Error scheduling task: {}", e),
    }
}

/// Cancels a pending task by its ID.
/// Expects args to be { "task_id": number }
pub async fn cancel_task(
    args: Value,
    task_manager: std::sync::Arc<crate::tasks::TaskManager>,
) -> String {

    let task_id_str = match args.pointer("/task_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return "Error: 'task_id' field missing or not a string".to_string(),
    };

    let task_id = match uuid::Uuid::parse_str(task_id_str) {
        Ok(id) => id,
        Err(_) => return "Error: 'task_id' field is not a valid UUID".to_string(),
    };

    log::info!("Canceling task #{}", task_id);

    match task_manager.cancel_task(task_id).await {
        Ok(_) => {
            log::info!("Successfully canceled task #{}", task_id);
            format!("Successfully canceled task ID: {}", task_id)
        }
        Err(e) => format!("Error canceling task: {}", e),
    }
}

/// Lists all currently scheduled and pending tasks.
pub async fn list_tasks(
    _args: Value,
    task_manager: std::sync::Arc<crate::tasks::TaskManager>,
) -> String {
    log::info!("Listing all pending tasks");
    let tasks = task_manager.list_tasks().await;
    match serde_json::to_string_pretty(&tasks) {
        Ok(json_str) => json_str,
        Err(e) => format!("Error serializing task list: {}", e),
    }
}

/// Adds a new memory string into the vector database.
pub async fn add_memory(
    args: Value,
    embedding: &crate::agent::Embedding,
    memory_store: &crate::agent::MemoryStore,
) -> String {

    let text = match args.pointer("/text").and_then(|v| v.as_str()) {
        Some(t) => t.to_string(),
        None => return "Error: 'text' field missing or not a string".to_string(),
    };

    log::info!("Adding memory: {}", text);

    let doc_embedding = match embedding.embed_doc(&text).await {
        Ok(e) => e,
        Err(e) => return format!("Error generating embedding: {}", e),
    };

    match memory_store.add_memory(&text, &doc_embedding) {
        Ok(id) => format!("Successfully added memory with ID: {}", id),
        Err(e) => format!("Error storing memory: {}", e),
    }
}

/// Removes a memory from the vector database by ID.
pub async fn remove_memory(args: Value, memory_store: &crate::agent::MemoryStore) -> String {

    let id = match args.pointer("/id").and_then(|v| v.as_u64()) {
        Some(id) => id as i64,
        None => return "Error: 'id' field missing or not a positive integer".to_string(),
    };

    log::info!("Removing memory #{}", id);

    match memory_store.remove_memory(id) {
        Ok(_) => format!("Successfully removed memory ID: {}", id),
        Err(e) => format!("Error removing memory: {}", e),
    }
}

/// Searches for relevant memories using semantic similarity.
pub async fn search_memory(
    args: Value,
    embedding: &crate::agent::Embedding,
    memory_store: &crate::agent::MemoryStore,
) -> String {

    let query = match args.pointer("/query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return "Error: 'query' field missing or not a string".to_string(),
    };

    let top_k = match args.pointer("/top_k").and_then(|v| v.as_u64()) {
        Some(k) => k as usize,
        None => return "Error: 'top_k' field missing or not a positive integer".to_string(),
    };

    log::info!("Searching memories for: {} (top_k: {})", query, top_k);

    let query_embedding = match embedding.embed_query(&query).await {
        Ok(e) => e,
        Err(e) => return format!("Error generating query embedding: {}", e),
    };

    match memory_store.search_memory(&query_embedding, top_k) {
        Ok(results) => {
            if results.is_empty() {
                "No relevant memories found.".to_string()
            } else {
                let mut output = format!("Found {} memories:\n", results.len());
                for (i, (id, text, sim)) in results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. [ID: {}] {} (Similarity: {:.4})\n",
                        i + 1,
                        id,
                        text,
                        sim
                    ));
                }
                output
            }
        }
        Err(e) => format!("Error searching memories: {}", e),
    }
}
