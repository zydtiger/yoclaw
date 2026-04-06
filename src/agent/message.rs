use super::{Message, MessageHistory, Role};

impl Message {
    pub fn new(role: Role, content: String) -> Self {
        Self {
            role,
            content: Some(content),
            reasoning_content: None,
            name: None,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_tool_call_id(mut self, tool_call_id: String) -> Self {
        self.tool_call_id = Some(tool_call_id);
        self
    }
}

impl MessageHistory {
    pub fn new(entries: Vec<Message>) -> Self {
        Self {
            entries,
            total_tokens: 0,
        }
    }

    pub fn compact_task_messages(&mut self, task_start_offset: usize, assistant_content: String) {
        let user_message = self.entries.get(task_start_offset).cloned();

        self.entries.truncate(task_start_offset);

        if let Some(user_message) = user_message {
            self.entries.push(user_message);
        } else {
            log::warn!(
                "Missing task user message at offset {}; compacting with assistant response only",
                task_start_offset
            );
        }

        self.entries
            .push(Message::new(Role::Assistant, assistant_content));
    }

    pub fn clear_preserving_system(&mut self) {
        if self
            .entries
            .first()
            .is_some_and(|message| message.role == Role::System)
        {
            self.entries.truncate(1);
        } else {
            log::warn!("Message history missing system prompt; clearing all messages");
            self.entries.clear();
        }

        self.total_tokens = 0;
    }
}
