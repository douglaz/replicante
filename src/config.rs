use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::llm::LLMConfig;
use crate::mcp::MCPServerConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub agent: AgentConfig,
    pub llm: LLMConfig,
    pub mcp_servers: Vec<MCPServerConfig>,
    pub database_path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentConfig {
    pub id: Option<String>,
    pub log_level: Option<String>,
    pub initial_goals: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        // Check for config path from environment variable or command line
        let config_path = std::env::var("CONFIG_FILE")
            .unwrap_or_else(|_| {
                // Check command line arguments
                let args: Vec<String> = std::env::args().collect();
                if let Some(config_idx) = args.iter().position(|arg| arg == "--config") {
                    if config_idx + 1 < args.len() {
                        return args[config_idx + 1].clone();
                    }
                }
                "config.toml".to_string()
            });
        
        // Try to load from file
        if Path::new(&config_path).exists() {
            let contents = fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&contents)?;
            return Ok(config);
        }

        // Fall back to default configuration
        Ok(Self::default())
    }

    pub fn default() -> Self {
        Self {
            agent: AgentConfig {
                id: Some("replicante-001".to_string()),
                log_level: Some("info".to_string()),
                initial_goals: None,
            },
            llm: LLMConfig {
                provider: std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "anthropic".to_string()),
                api_key: None, // Will be loaded from environment
                model: std::env::var("LLM_MODEL")
                    .unwrap_or_else(|_| "claude-3-opus-20240229".to_string()),
                temperature: Some(0.7),
                max_tokens: Some(4000),
                api_url: None,
            },
            mcp_servers: vec![
                MCPServerConfig {
                    name: "nostr".to_string(),
                    transport: "stdio".to_string(),
                    command: "mcp-server-nostr".to_string(),
                    args: vec!["--relay".to_string(), "wss://relay.damus.io".to_string()],
                },
                MCPServerConfig {
                    name: "filesystem".to_string(),
                    transport: "stdio".to_string(),
                    command: "mcp-server-filesystem".to_string(),
                    args: vec!["--root".to_string(), "/data".to_string()],
                },
                MCPServerConfig {
                    name: "http".to_string(),
                    transport: "stdio".to_string(),
                    command: "mcp-server-http".to_string(),
                    args: vec![],
                },
                MCPServerConfig {
                    name: "bitcoin".to_string(),
                    transport: "stdio".to_string(),
                    command: "mcp-server-bitcoin".to_string(),
                    args: vec![],
                },
            ],
            database_path: std::env::var("DATABASE_PATH")
                .unwrap_or_else(|_| "replicante.db".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.agent.id.is_some());
        assert!(!config.mcp_servers.is_empty());
    }
}
