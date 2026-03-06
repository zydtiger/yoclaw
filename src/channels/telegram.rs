use reqwest::Client;
use serde::Deserialize;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Channel, ChannelMessage};

const TELEGRAM_API_URL: &str = "https://api.telegram.org";

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
}

impl Channel for TelegramChannel {
    async fn send_message(
        &self,
        recipient_id: &str,
        content: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("sendMessage");
        let req_body = serde_json::json!({
            "chat_id": recipient_id,
            "text": content,
            "parse_mode": "MarkdownV2",
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
            return Err(format!("Telegram API error: {:?}", response.description).into());
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

                    messages.push(ChannelMessage {
                        channel_id: "telegram".to_string(),
                        chat_id,
                        sender_id,
                        sender_name,
                        text,
                    });
                }
            }
        }

        // Update the lowest unconfirmed update ID
        self.last_update_id
            .store(highest_update_id, Ordering::SeqCst);

        Ok(messages)
    }
}
