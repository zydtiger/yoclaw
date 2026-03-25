# Creating Skills

When creating skills, write to `~/.yoclaw` following the standard skill conventions. Create a directory with the skill name, then create `SKILL.md` within the directory to present the contents with name and description metadata at the top of the file. If there is a need for scripts, write `uv run --script` format in `skill_dir/scripts` directory.

## Skill Structure

A skill is organized as follows:

```
skill-name/
  SKILL.md          # Required: skill definition and instructions
  scripts/          # Optional: executable scripts
    script-name.py  # uv run --script scripts/script-name.py
```

## SKILL.md Format

The `SKILL.md` file uses a YAML frontmatter block for metadata:

```markdown
---
name: Skill Name
description: A brief description of what this skill does
version: 1.0.0
---

# Skill Name

## Description

A detailed description of the skill's purpose and functionality.

## Usage

Instructions on how and when to use this skill.

## Examples

Examples of typical usage scenarios.
```

### Sample SKILL.md

```markdown
---
name: daily-summary
description: Generates a daily summary of tasks and activities
version: 1.0.0
---

# Daily Summary

## Description

This skill generates a comprehensive daily summary of scheduled tasks, completed items, and upcoming deadlines. It aggregates information from the task manager and provides a formatted report.

## Usage

Invoke this skill when a daily overview is needed. The skill will:
1. Query all tasks due today
2. Filter completed and pending items
3. Generate a formatted summary report

## Examples

- "Generate my daily summary"
- "What's on my schedule for today?"
```

## uv Script Format

Scripts within the `scripts/` directory should use `uv run --script` format with self-contained dependencies specified via inline comments:

```python
#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "requests>=2.31.0",
#     "pydantic>=2.5.0",
# ]
# ///

import requests
from pydantic import BaseModel

class Task(BaseModel):
    id: int
    title: str
    status: str

def fetch_tasks():
    response = requests.get("https://api.example.com/tasks")
    response.raise_for_status()
    return [Task(**item) for item in response.json()]

if __name__ == "__main__":
    tasks = fetch_tasks()
    for task in tasks:
        print(f"{task.id}: {task.title} - {task.status}")
```

### Key Points

1. **Frontmatter**: The YAML frontmatter block is delimited by `---` and must contain at minimum the `name` field. `description` and `version` are optional.

2. **Self-contained Dependencies**: Scripts specify all dependencies inline, ensuring they can run in isolation without a separate virtual environment setup.

3. **Script Location**: Place executable scripts in `scripts/` subdirectory. Each script should be invocable via `uv run --script scripts/script-name.py`.

4. **Directory Name**: The skill directory name is used as the default skill identifier if no name is provided in the frontmatter.