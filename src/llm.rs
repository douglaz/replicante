use anyhow::{Result, bail};
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
    pub timeout_secs: Option<u64>,
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
        "mock" => Ok(Box::new(MockLLMProvider::new())),
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
            .or_else(|| std::env::var("GEMINI_API_KEY").ok())
            .ok_or_else(|| anyhow::anyhow!("OpenAI/Gemini API key not found"))?;

        let mut api_url = config
            .api_url
            .clone()
            .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());

        // Fix Gemini endpoint if needed
        if api_url.contains("generativelanguage.googleapis.com")
            && !api_url.contains("/chat/completions")
        {
            api_url = format!("{}/chat/completions", api_url.trim_end_matches('/'));
        }

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

        // Use configured timeout, or smart defaults based on model size
        let timeout_secs = config.timeout_secs.unwrap_or_else(|| {
            if config.model.contains("70b") || config.model.contains("405b") {
                1800 // 30 minutes for large models
            } else {
                300 // 5 minutes for regular models
            }
        });

        tracing::info!(
            "Ollama provider initialized for model '{}' with {} second timeout",
            config.model,
            timeout_secs
        );

        Ok(Self {
            client: Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
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

// Mock LLM Provider for testing
pub struct MockLLMProvider {
    response_counter: std::sync::Arc<std::sync::Mutex<usize>>,
}

impl MockLLMProvider {
    pub fn new() -> Self {
        Self {
            response_counter: std::sync::Arc::new(std::sync::Mutex::new(0)),
        }
    }
}

impl Default for MockLLMProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LLMProvider for MockLLMProvider {
    async fn complete(&self, _prompt: &str) -> Result<String> {
        let mut counter = self.response_counter.lock().unwrap();
        *counter += 1;

        // Return different responses based on call count to simulate reasoning
        let response = match *counter {
            1 => {
                // First call - return exploration action
                r#"{
                    "reasoning": "I should explore my environment to understand my capabilities",
                    "confidence": 0.9,
                    "proposed_actions": ["explore"]
                }"#
            }
            2 => {
                // Second call - use a tool
                r#"{
                    "reasoning": "I should list the directory contents to see what's available",
                    "confidence": 0.85,
                    "proposed_actions": ["use_tool:filesystem:list_directory"]
                }"#
            }
            3 => {
                // Third call - remember something
                r#"{
                    "reasoning": "I should remember what I've learned about my environment",
                    "confidence": 0.8,
                    "proposed_actions": ["remember:test_key:test_value"]
                }"#
            }
            _ => {
                // Default - wait
                r#"{
                    "reasoning": "I should wait and observe",
                    "confidence": 0.7,
                    "proposed_actions": ["wait"]
                }"#
            }
        };

        Ok(response.to_string())
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
            timeout_secs: None,
        };

        let _provider = create_provider(&config)?;
        // Provider created successfully
        Ok(())
    }

    #[test]
    fn test_create_mock_provider() -> Result<()> {
        let config = LLMConfig {
            provider: "mock".to_string(),
            api_key: None,
            model: "mock".to_string(),
            temperature: None,
            max_tokens: None,
            api_url: None,
            timeout_secs: None,
        };

        let _provider = create_provider(&config)?;
        // Mock provider created successfully
        Ok(())
    }

    #[tokio::test]
    async fn test_mock_provider_responses() -> Result<()> {
        let provider = MockLLMProvider::new();

        // First response should be explore
        let response1 = provider.complete("test prompt").await?;
        assert!(response1.contains("explore"));

        // Second response should use tool
        let response2 = provider.complete("test prompt").await?;
        assert!(response2.contains("use_tool"));

        // Third response should remember
        let response3 = provider.complete("test prompt").await?;
        assert!(response3.contains("remember"));

        Ok(())
    }
}
