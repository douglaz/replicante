use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tracing::{debug, info};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MCPServerConfig {
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<Value>,
}

pub struct MCPClient {
    servers: Vec<MCPServer>,
}

struct MCPServer {
    name: String,
    process: Option<Child>,
    tools: Vec<Tool>,
}

impl MCPClient {
    pub async fn new(configs: &[MCPServerConfig]) -> Result<Self> {
        let mut servers = Vec::new();

        for config in configs {
            info!("Initializing MCP server: {}", config.name);

            // For now, create placeholder servers
            // In production, would spawn actual MCP server processes
            let server = MCPServer {
                name: config.name.clone(),
                process: None,
                tools: Vec::new(),
            };

            servers.push(server);
        }

        // Initialize with some mock tools for testing
        if !servers.is_empty() {
            // Mock Nostr tools
            if servers.iter().any(|s| s.name.contains("nostr")) {
                if let Some(server) = servers.iter_mut().find(|s| s.name.contains("nostr")) {
                    server.tools.push(Tool {
                        name: "nostr_publish".to_string(),
                        description: Some("Publish a message to Nostr".to_string()),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "content": { "type": "string" },
                                "tags": { "type": "array" }
                            }
                        })),
                    });
                    server.tools.push(Tool {
                        name: "nostr_subscribe".to_string(),
                        description: Some("Subscribe to Nostr events".to_string()),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "filters": { "type": "array" }
                            }
                        })),
                    });
                }
            }

            // Mock filesystem tools
            if servers.iter().any(|s| s.name.contains("filesystem")) {
                if let Some(server) = servers.iter_mut().find(|s| s.name.contains("filesystem")) {
                    server.tools.push(Tool {
                        name: "fs_read".to_string(),
                        description: Some("Read a file".to_string()),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" }
                            }
                        })),
                    });
                    server.tools.push(Tool {
                        name: "fs_write".to_string(),
                        description: Some("Write to a file".to_string()),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "path": { "type": "string" },
                                "content": { "type": "string" }
                            }
                        })),
                    });
                }
            }

            // Mock HTTP tools
            if servers.iter().any(|s| s.name.contains("http")) {
                if let Some(server) = servers.iter_mut().find(|s| s.name.contains("http")) {
                    server.tools.push(Tool {
                        name: "http_get".to_string(),
                        description: Some("Make an HTTP GET request".to_string()),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "url": { "type": "string" }
                            }
                        })),
                    });
                    server.tools.push(Tool {
                        name: "http_post".to_string(),
                        description: Some("Make an HTTP POST request".to_string()),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "url": { "type": "string" },
                                "body": { "type": "object" }
                            }
                        })),
                    });
                }
            }

            // Mock Bitcoin/Lightning tools
            if servers.iter().any(|s| s.name.contains("bitcoin")) {
                if let Some(server) = servers.iter_mut().find(|s| s.name.contains("bitcoin")) {
                    server.tools.push(Tool {
                        name: "lightning_invoice".to_string(),
                        description: Some("Create a Lightning invoice".to_string()),
                        parameters: Some(serde_json::json!({
                            "type": "object",
                            "properties": {
                                "amount_sats": { "type": "number" },
                                "description": { "type": "string" }
                            }
                        })),
                    });
                    server.tools.push(Tool {
                        name: "check_balance".to_string(),
                        description: Some("Check wallet balance".to_string()),
                        parameters: None,
                    });
                }
            }
        }

        Ok(Self { servers })
    }

    pub async fn list_tools(&self) -> Result<Vec<String>> {
        let mut all_tools = Vec::new();

        for server in &self.servers {
            for tool in &server.tools {
                all_tools.push(format!("{}:{}", server.name, tool.name));
            }
        }

        Ok(all_tools)
    }

    pub async fn discover_tools(&mut self) -> Result<Vec<Tool>> {
        let mut all_tools = Vec::new();

        for server in &mut self.servers {
            // In production, would query the MCP server for available tools
            // For now, return the mock tools
            all_tools.extend(server.tools.clone());
        }

        Ok(all_tools)
    }

    pub async fn use_tool(&self, name: &str, params: Value) -> Result<Value> {
        debug!("Using tool: {} with params: {:?}", name, params);

        // Parse server:tool format
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() != 2 {
            bail!("Invalid tool name format. Expected 'server:tool'");
        }

        let server_name = parts[0];
        let tool_name = parts[1];

        // Find the server
        let server = self
            .servers
            .iter()
            .find(|s| s.name == server_name)
            .ok_or_else(|| anyhow::anyhow!("Server not found: {}", server_name))?;

        // Find the tool
        let _tool = server
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;

        // Mock tool execution
        // In production, would send request to MCP server
        match tool_name {
            "nostr_publish" => Ok(serde_json::json!({
                "success": true,
                "event_id": uuid::Uuid::new_v4().to_string(),
                "message": "Published to Nostr"
            })),
            "fs_read" => Ok(serde_json::json!({
                "success": true,
                "content": "File content here"
            })),
            "http_get" => Ok(serde_json::json!({
                "success": true,
                "status": 200,
                "body": "Response body"
            })),
            "lightning_invoice" => Ok(serde_json::json!({
                "success": true,
                "invoice": "lnbc1234...",
                "payment_hash": uuid::Uuid::new_v4().to_string()
            })),
            "check_balance" => Ok(serde_json::json!({
                "success": true,
                "balance_sats": 100000
            })),
            _ => Ok(serde_json::json!({
                "success": false,
                "error": "Tool not implemented"
            })),
        }
    }

    // In production, this would spawn actual MCP server processes
    async fn start_server(config: &MCPServerConfig) -> Result<Child> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let child = cmd.spawn()?;
        Ok(child)
    }
}

impl Drop for MCPClient {
    fn drop(&mut self) {
        // Clean up any running MCP server processes
        for server in &mut self.servers {
            if let Some(mut process) = server.process.take() {
                let _ = process.kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mcp_client_creation() -> Result<()> {
        let configs = vec![MCPServerConfig {
            name: "test".to_string(),
            transport: "stdio".to_string(),
            command: "echo".to_string(),
            args: vec!["test".to_string()],
        }];

        let client = MCPClient::new(&configs).await?;
        assert_eq!(client.servers.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_list_tools() -> Result<()> {
        let configs = vec![MCPServerConfig {
            name: "nostr".to_string(),
            transport: "stdio".to_string(),
            command: "mcp-server-nostr".to_string(),
            args: vec![],
        }];

        let client = MCPClient::new(&configs).await?;
        let tools = client.list_tools().await?;
        assert!(!tools.is_empty());
        Ok(())
    }
}
