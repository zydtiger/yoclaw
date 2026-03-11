use super::Embedding;
use reqwest::Result;
use serde_json::json;

impl Embedding {
    pub fn new(
        config: &crate::config::EmbeddingConfig,
    ) -> std::result::Result<Self, url::ParseError> {
        let api_url = reqwest::Url::parse(&config.openai_api_base_url)?;
        Ok(Self {
            api_url,
            api_key: config.openai_api_key.clone(),
            model: config.openai_model.clone(),
            client: reqwest::Client::new(),
        })
    }

    pub async fn embed_doc(&self, text: &str) -> Result<Vec<f32>> {
        self.get_embedding(&format!("Represent this document for searching: {}", text))
            .await
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.get_embedding(&format!(
            "Represent this query for retrieving documents: {}",
            text
        ))
        .await
    }

    async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let request_body = json!({
            "model": self.model,
            "input": text,
        });

        let response = self
            .client
            .post(format!("{}/embeddings", self.api_url))
            .bearer_auth(&self.api_key)
            .json(&request_body)
            .send()
            .await?;

        let response_json: serde_json::Value = response.json().await?;

        // Assuming standard OpenAI-compatible response format:
        // { "data": [ { "embedding": [0.1, 0.2, ...] } ] }
        let embedding = response_json["data"][0]["embedding"]
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .map(|v| v.as_f64().unwrap_or(0.0) as f32)
            .collect();

        Ok(embedding)
    }
}
