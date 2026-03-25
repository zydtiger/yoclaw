use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Channel, ChannelMessage};

const TELEGRAM_API_URL: &str = "https://api.telegram.org";
const MAX_MESSAGE_LEN: usize = 4_096; // Telegram's message length limit

#[derive(Debug, Deserialize)]
struct TelegramResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: u64,
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    from: Option<TelegramPeer>,
    chat: TelegramPeer,
    text: Option<String>,
    message_id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramPeer {
    id: i64,
    #[serde(alias = "first_name", alias = "title")]
    name: Option<String>,
}

pub struct TelegramChannel {
    token: String,
    client: Client,
    last_update_id: AtomicU64,
}

impl TelegramChannel {
    pub fn new(token: String) -> Self {
        Self {
            token,
            client: Client::new(),
            last_update_id: AtomicU64::new(0),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", TELEGRAM_API_URL, self.token, method)
    }

    fn truncate_content(&self, content: &str) -> String {
        if content.len() > MAX_MESSAGE_LEN {
            log::warn!(
                "Message exceeds MAX_MESSAGE_LEN ({} > {}). Truncating to {} bytes.",
                content.len(),
                MAX_MESSAGE_LEN,
                MAX_MESSAGE_LEN
            );
            let warning_msg = "\n...TOO LONG FOR TELEGRAM";
            content[..MAX_MESSAGE_LEN - warning_msg.len()].to_string() + warning_msg
        } else {
            content.to_string()
        }
    }

    async fn send_telegram_message(
        &self,
        recipient_id: &str,
        content: &str,
        parse_mode: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("sendMessage");
        let mut req_body = serde_json::json!({
            "chat_id": recipient_id,
            "text": self.truncate_content(content),
        });

        if let Some(parse_mode) = parse_mode {
            req_body["parse_mode"] = serde_json::Value::String(parse_mode.to_string());
        }

        let response = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await?
            .json::<TelegramResponse<serde_json::Value>>()
            .await?;

        if !response.ok {
            return Err(format!("Telegram API error: {:?}", response.description).into());
        }

        Ok(())
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    async fn send_message(
        &self,
        recipient_id: &str,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Convert standard Markdown to Telegram's MarkdownV2 dialect
        let md_v2_content = match telegram_markdown_v2::convert_with_strategy(
            content,
            telegram_markdown_v2::UnsupportedTagsStrategy::Escape,
        ) {
            Ok(content) => content,
            Err(e) => {
                log::error!("Failed to convert markdown to MarkdownV2: {}", e);
                self.send_telegram_message(recipient_id, "Failed to send MarkdownV2", None)
                    .await?;
                return self
                    .send_telegram_message(recipient_id, content, None)
                    .await;
            }
        };

        if let Err(e) = self
            .send_telegram_message(recipient_id, &md_v2_content, Some("MarkdownV2"))
            .await
        {
            log::error!("Failed to send MarkdownV2 message: {}", e);
            self.send_telegram_message(recipient_id, "Failed to send MarkdownV2", None)
                .await?;
            return self
                .send_telegram_message(recipient_id, content, None)
                .await;
        }

        Ok(())
    }

    async fn receive_messages(
        &self,
    ) -> Result<Vec<ChannelMessage>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("getUpdates");

        let offset = self.last_update_id.load(Ordering::SeqCst);
        let req_body = serde_json::json!({
            // If offset is 0, we don't send it, so it fetches the last unconfirmed messages
            // Alternatively, Telegram recommends offset = last_update_id + 1 to confirm receipt
            "offset": if offset == 0 { None } else { Some(offset + 1) },
            "timeout": 10, // Long polling timeout in seconds
        });

        let response = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await?
            .json::<TelegramResponse<Vec<TelegramUpdate>>>()
            .await?;

        if !response.ok {
            return Err(format!("Telegram API error: {:?}", response.description).into());
        }

        let updates = response.result.unwrap_or_default();
        let mut messages = Vec::new();
        let mut highest_update_id = offset;

        for update in updates {
            if update.update_id > highest_update_id {
                highest_update_id = update.update_id;
            }

            if let Some(msg) = update.message {
                if let Some(text) = msg.text {
                    let sender_name = msg.from.as_ref().and_then(|f| f.name.clone());
                    let sender_id = msg.from.map(|f| f.id.to_string()).unwrap_or_default();
                    let chat_id = msg.chat.id.to_string();
                    let message_id = msg.message_id;

                    messages.push(ChannelMessage {
                        channel_id: "telegram".to_string(),
                        chat_id,
                        sender_id,
                        sender_name,
                        text,
                        message_id,
                    });
                }
            }
        }

        // Update the lowest unconfirmed update ID
        self.last_update_id
            .store(highest_update_id, Ordering::SeqCst);

        Ok(messages)
    }

    async fn react_with_emoji(
        &self,
        chat_id: &str,
        message_id: i64,
        emoji: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("setMessageReaction");

        let req_body = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "reaction": [
                {
                    "type": "emoji",
                    "emoji": emoji
                }
            ]
        });

        let response = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await?
            .json::<TelegramResponse<serde_json::Value>>()
            .await?;

        if !response.ok {
            return Err(format!(
                "Telegram API error adding reaction: {:?}",
                response.description
            )
            .into());
        }

        Ok(())
    }

    async fn register_commands(
        &self,
        commands: Vec<super::BotCommand>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("setMyCommands");

        let req_body = serde_json::json!({
            "commands": commands
        });

        let response = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await?
            .json::<TelegramResponse<serde_json::Value>>()
            .await?;

        if !response.ok {
            return Err(format!(
                "Telegram API error setting commands: {:?}",
                response.description
            )
            .into());
        }

        Ok(())
    }
}
