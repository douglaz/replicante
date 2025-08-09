use anyhow::{bail, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::debug;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LLMConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub model: String,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub api_url: Option<String>,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String>;
}

pub fn create_provider(config: &LLMConfig) -> Result<Box<dyn LLMProvider>> {
    match config.provider.to_lowercase().as_str() {
        "anthropic" => Ok(Box::new(AnthropicProvider::new(config)?)),
        "openai" => Ok(Box::new(OpenAIProvider::new(config)?)),
        "ollama" => Ok(Box::new(OllamaProvider::new(config)?)),
        _ => bail!("Unknown LLM provider: {}", config.provider),
    }
}

// Anthropic Claude Provider
struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    temperature: f64,
    max_tokens: u32,
}

impl AnthropicProvider {
    fn new(config: &LLMConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("Anthropic API key not found"))?;

        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()?,
            api_key,
            model: config.model.clone(),
            temperature: config.temperature.unwrap_or(0.7),
            max_tokens: config.max_tokens.unwrap_or(4000),
        })
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        debug!("Sending request to Anthropic API");

        let request_body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": prompt
            }],
            "max_tokens": self.max_tokens,
            "temperature": self.temperature
        });

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            bail!("Anthropic API error: {}", error_text);
        }

        let response_json: serde_json::Value = response.json().await?;

        let content = response_json["content"][0]["text"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid response from Anthropic API"))?;

        Ok(content.to_string())
    }
}

// OpenAI Provider
struct OpenAIProvider {
    client: Client,
    api_key: String,
    model: String,
    temperature: f64,
    max_tokens: u32,
    api_url: String,
}

impl OpenAIProvider {
    fn new(config: &LLMConfig) -> Result<Self> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("OpenAI API key not found"))?;

        let api_url = config
            .api_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());

        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()?,
            api_key,
            model: config.model.clone(),
            temperature: config.temperature.unwrap_or(0.7),
            max_tokens: config.max_tokens.unwrap_or(4000),
            api_url,
        })
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        debug!("Sending request to OpenAI API");

        let request_body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": prompt
            }],
            "max_tokens": self.max_tokens,
            "temperature": self.temperature
        });

        let response = self
            .client
            .post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            bail!("OpenAI API error: {}", error_text);
        }

        let response_json: serde_json::Value = response.json().await?;

        let content = response_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid response from OpenAI API"))?;

        Ok(content.to_string())
    }
}

// Ollama Provider (for local models)
struct OllamaProvider {
    client: Client,
    model: String,
    api_url: String,
}

impl OllamaProvider {
    fn new(config: &LLMConfig) -> Result<Self> {
        let api_url = config
            .api_url
            .clone()
            .or_else(|| std::env::var("OLLAMA_HOST").ok())
            .unwrap_or_else(|| "http://localhost:11434".to_string());

        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(300))
                .build()?,
            model: config.model.clone(),
            api_url,
        })
    }
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    async fn complete(&self, prompt: &str) -> Result<String> {
        debug!("Sending request to Ollama API");

        let request_body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false
        });

        let response = self
            .client
            .post(format!("{}/api/generate", self.api_url))
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            bail!("Ollama API error: {}", error_text);
        }

        let response_json: serde_json::Value = response.json().await?;

        let content = response_json["response"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid response from Ollama API"))?;

        Ok(content.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_provider() -> Result<()> {
        let config = LLMConfig {
            provider: "anthropic".to_string(),
            api_key: Some("test-key".to_string()),
            model: "claude-3-opus-20240229".to_string(),
            temperature: Some(0.7),
            max_tokens: Some(4000),
            api_url: None,
        };

        let _provider = create_provider(&config)?;
        // Provider created successfully
        Ok(())
    }
}
