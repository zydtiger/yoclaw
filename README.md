# Yoclaw

Yoclaw is a Telegram-first LLM agent written in Rust. It talks to OpenAI-compatible chat and embedding APIs, supports tool calling, keeps a persistent task queue, and stores semantic memories in SQLite with `sqlite-vec`.

## Features

- Telegram bot interface with `/help`, `/tools`, and `/skills`
- OpenAI-compatible chat completion backend for the main agent
- OpenAI-compatible embeddings backend for semantic memory search
- Built-in tools for shell commands, file I/O, URL fetches, scheduling, and memory operations
- Persistent scheduled tasks in `tasks.json`
- Persistent task routing in `routes.json`
- Optional skill loading from `~/.yoclaw/skills` or `CONFIG_PATH/skills`

## Quick Start

### Option 1: Run with Cargo

1. Build and run once to generate the default config template:

```bash
cargo run
```

This creates `~/.yoclaw/config.toml` by default if it does not already exist.

2. Edit the config:

```toml
[agent]
openai_api_base_url = "http://localhost:11434/v1"
openai_api_key = "ollama"
openai_model = "qwen3-4b"
system_prompt = "Your name is YoClaw, and you are a helpful assistant."
debug_mode = false

[embedding]
openai_api_base_url = "http://localhost:11434/v1"
openai_api_key = "ollama"
openai_model = "qwen3-embedding:8b"

[channels]
telegram_token = "<telegram bot token>"
allowed_users = ["<telegram user id>"]
recv_confirm = "👍"
```

3. Start the bot:

```bash
cargo run --release
```

### Install Skills from the CLI

You can manage local skills without starting the bot runtime:

```bash
cargo run -- skill add dir:/path/to/skill
cargo run -- skill add zip:/path/to/skill.zip
cargo run -- skill add zip:https://example.com/skill.zip
```

Installed skills are copied into `CONFIG_PATH/skills` (or `~/.yoclaw/skills` by default).

### Option 2: Run in Docker or Podman

1. Create the config first, either by running `cargo run` once or by writing `~/.yoclaw/config.toml` manually.

2. Build the image:

```bash
docker build -t yoclaw:latest -f Containerfile .
```

Or with Podman:

```bash
podman build -t localhost/yoclaw:latest -f Containerfile .
```

3. Run the container:

```bash
docker run --rm \
  -v "$HOME/.yoclaw:/root/.yoclaw" \
  yoclaw:latest
```

With Podman:

```bash
podman run --rm \
  --network host \
  -v /etc/localtime:/etc/localtime:ro \
  -v "$HOME/.yoclaw:/root/.yoclaw:Z" \
  localhost/yoclaw:latest
```

### Option 3: Run as a User Service with Podman Quadlet

The repo includes [yoclaw.container](yoclaw.container), which generates `yoclaw.service`.

1. Build the image:

```bash
podman build -t localhost/yoclaw:latest -f Containerfile .
```

2. Install the Quadlet file:

```bash
mkdir -p ~/.config/containers/systemd
cp yoclaw.container ~/.config/containers/systemd/
```

3. Reload and start the user service:

```bash
systemctl --user daemon-reload
systemctl --user enable --now yoclaw.service
```

## Configuration

Yoclaw reads its config directory from `CONFIG_PATH`. If `CONFIG_PATH` is unset, it defaults to `~/.yoclaw`.

The main config file is:

```text
~/.yoclaw/config.toml
```

The default template comes from [src/config/template/config.toml](src/config/template/config.toml).

### Config fields

- `[agent]`
  - `openai_api_base_url`: base URL for the chat API, for example `http://localhost:11434/v1`
  - `openai_api_key`: bearer token sent to the chat API
  - `openai_model`: chat model name
  - `system_prompt`: initial system prompt
  - `debug_mode`: when `true`, tool-call progress and usage data are sent back in responses
- `[embedding]`
  - `openai_api_base_url`: base URL for the embeddings API
  - `openai_api_key`: bearer token sent to the embeddings API
  - `openai_model`: embedding model name
- `[channels]`
  - `telegram_token`: Telegram bot token
  - `allowed_users`: list of allowed Telegram user IDs as strings
  - `recv_confirm`: optional emoji reaction added to accepted incoming messages
- `[environment]`
  - optional environment variables exposed to the `generic_shell` tool

### Important behavior

- `allowed_users = []` blocks everyone. The bot replies with a warning that includes the sender's Telegram user ID.
- Chat completions are sent to `{openai_api_base_url}/chat/completions`.
- Embeddings are sent to `{embedding.openai_api_base_url}/embeddings`.

## Runtime Layout

The runtime in [src/main.rs](src/main.rs) is split into four long-lived async flows:

- shutdown signal listener
- Telegram listener
- Telegram sender
- `TaskProcessor`

The agent itself stays on the main thread. This keeps agent state single-owner and avoids deadlocks between tool calls and task management.

Inbound Telegram messages are turned into tasks, executed by the agent, and routed back to the originating chat through the sender loop.

## Built-In Tools

The built-in tool definitions live in [src/agent/tools/mod.rs](src/agent/tools/mod.rs). The current tool surface is:

- `get_current_time`
- `generic_shell`
- `use_skill`
- `read_file`
- `write_file`
- `get_url`
- `schedule_task`
- `cancel_task`
- `list_tasks`
- `add_memory`
- `remove_memory`
- `search_memory`

## Scheduled Tasks

Tasks are managed by [src/tasks/processor.rs](src/tasks/processor.rs) and [src/tasks/manager.rs](src/tasks/manager.rs).

- Immediate messages and scheduled jobs share the same task pipeline.
- Repeating tasks support `daily` and `weekly`.
- Repeating tasks reschedule from the original deadline anchor, not from completion time.
- Pending tasks are persisted in `tasks.json` in the config directory.

## Memory and Skills

- Semantic memory is stored in `memory.db` under the config directory.
- Skills are loaded from `skills/` under the config directory.
- A skill can be either a subdirectory containing `SKILL.md` or a direct Markdown file.
- `yoclaw skill add ...` installs managed skills into that same `skills/` directory.

## Telegram Commands

The Telegram command registry is defined in [src/channels/command.rs](src/channels/command.rs):

- `/help`
- `/tools`
- `/skills`

## Data Files

By default, Yoclaw stores runtime state under `~/.yoclaw`:

- `config.toml`: main configuration
- `memory.db`: SQLite memory store
- `tasks.json`: persisted scheduled tasks
- `routes.json`: task-to-chat routing table
- `skills/`: optional skill definitions

## Development

Build:

```bash
cargo build
```

Run tests:

```bash
cargo test
```

Read the detailed architecture notes in [STRUCTURE.md](STRUCTURE.md).
