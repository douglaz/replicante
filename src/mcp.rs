use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, oneshot};
use tokio::time::{Duration, interval, timeout};
use tracing::{debug, error, info, warn};

use crate::jsonrpc::{Message, Request, RequestId, Response};
use crate::mcp_protocol::{
    ContentItem, InitializeParams, InitializeResult, ToolCallParams, ToolCallResult, ToolInfo,
    ToolsListResult,
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MCPServerConfig {
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,
    #[serde(default = "default_health_check_interval_secs")]
    pub health_check_interval_secs: u64,
}

fn default_retry_attempts() -> u32 {
    3
}
fn default_retry_delay_ms() -> u64 {
    1000
}
fn default_health_check_interval_secs() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<Value>,
}

pub struct MCPClient {
    servers: Vec<Arc<Mutex<MCPServer>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MCPServerHealth {
    pub name: String,
    pub is_healthy: bool,
    pub is_initialized: bool,
    pub last_healthy: Option<DateTime<Utc>>,
    pub error_count: u32,
    pub tool_count: usize,
}

struct MCPServer {
    name: String,
    config: MCPServerConfig,
    process: Option<Child>,
    stdin: Option<Arc<Mutex<tokio::process::ChildStdin>>>,
    tools: Vec<ToolInfo>,
    pending_requests: HashMap<RequestId, oneshot::Sender<Response>>,
    initialized: bool,
    last_healthy: Option<DateTime<Utc>>,
    error_count: u32,
    tools_cache_time: Option<DateTime<Utc>>,
}

impl MCPClient {
    pub async fn new(configs: &[MCPServerConfig]) -> Result<Self> {
        let mut servers = Vec::new();

        for config in configs {
            info!("Initializing MCP server: {name}", name = config.name);

            let server = Arc::new(Mutex::new(MCPServer {
                name: config.name.clone(),
                config: config.clone(),
                process: None,
                stdin: None,
                tools: Vec::new(),
                pending_requests: HashMap::new(),
                initialized: false,
                last_healthy: None,
                error_count: 0,
                tools_cache_time: None,
            }));

            // Start the server process with retries
            let mut attempts = 0;
            let max_attempts = config.retry_attempts;
            let retry_delay = Duration::from_millis(config.retry_delay_ms);

            loop {
                attempts += 1;
                match Self::start_server(server.clone()).await {
                    Ok(_) => {
                        info!(
                            "Successfully started MCP server: {name}",
                            name = config.name
                        );
                        break;
                    }
                    Err(e) => {
                        error!(
                            "Failed to start MCP server {name} (attempt {attempts}/{max_attempts}): {e}",
                            name = config.name
                        );

                        if attempts >= max_attempts {
                            warn!(
                                "Giving up on MCP server {name} after {max_attempts} attempts",
                                name = config.name
                            );
                            break;
                        }

                        tokio::time::sleep(retry_delay).await;
                    }
                }
            }

            servers.push(server);
        }

        // Start health monitoring
        let client = Self { servers };
        client.start_health_monitoring();
        Ok(client)
    }

    async fn start_server(server: Arc<Mutex<MCPServer>>) -> Result<()> {
        let mut server_guard = server.lock().await;
        let config = server_guard.config.clone();

        info!(
            "Starting MCP server process: {} {} (server: {})",
            config.command,
            config.args.join(" "),
            server_guard.name
        );

        // Spawn the MCP server process
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        debug!(
            "Spawning MCP server process: {command}",
            command = config.command
        );
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn MCP server: {command}",
                command = config.command
            )
        })?;

        debug!("MCP server process spawned with PID: {:?}", child.id());

        // Get handles to stdin/stdout
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdin handle"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdout handle"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stderr handle"))?;

        server_guard.process = Some(child);
        let server_name = server_guard.name.clone();

        // Store stdin writer in the server struct first
        let stdin = Arc::new(Mutex::new(stdin));
        server_guard.stdin = Some(stdin.clone());
        drop(server_guard); // Release lock before spawning tasks

        // Spawn task to handle stdout (JSON-RPC responses)
        let server_clone = server.clone();
        let server_name_stdout = server_name.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }

                debug!("Received from {server_name_stdout}: {line}");

                // Yield to allow other tasks to run
                tokio::task::yield_now().await;

                match Message::parse(&line) {
                    Ok(msg) => {
                        if let Err(e) = Self::handle_message(server_clone.clone(), msg).await {
                            error!("Failed to handle message: {e}");
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse JSON-RPC message: {e}");
                    }
                }

                // Yield again after processing
                tokio::task::yield_now().await;
            }

            warn!("MCP server {server_name_stdout} stdout closed");
        });

        // Spawn task to handle stderr (logging)
        let server_name_stderr = server_name.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                debug!("[{server_name_stderr}] {line}");
            }
        });

        // Wait a bit for the handlers to be ready
        debug!("Waiting for MCP server handlers to be ready...");
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Initialize the MCP connection with timeout
        debug!(
            "Starting MCP connection initialization for server: {}",
            server_name
        );
        match timeout(
            Duration::from_secs(10),
            Self::initialize_connection(server.clone(), stdin),
        )
        .await
        {
            Ok(Ok(())) => {
                debug!(
                    "MCP server initialization completed successfully: {}",
                    server_name
                );
            }
            Ok(Err(e)) => {
                error!("MCP server initialization failed: {server_name}: {e}");
                return Err(e);
            }
            Err(_) => {
                error!("MCP server initialization timed out: {server_name}");
                bail!("MCP server initialization timed out");
            }
        }

        Ok(())
    }

    async fn initialize_connection(
        server: Arc<Mutex<MCPServer>>,
        stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    ) -> Result<()> {
        let server_guard = server.lock().await;
        let server_name = server_guard.name.clone();
        drop(server_guard);

        // Send initialize request
        debug!("Sending initialize request to MCP server: {server_name}");
        let init_params = InitializeParams::new(
            "replicante".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );

        let request = Request::new("initialize", Some(serde_json::to_value(init_params)?));
        debug!(
            "Initialize request created for {}: {:?}",
            server_name, request.method
        );
        let response = Self::send_request(server.clone(), stdin.clone(), request).await?;
        debug!("Initialize response received from {server_name}");

        // Parse initialize response
        if let Some(result) = response.result {
            let init_result: InitializeResult = serde_json::from_value(result)?;
            info!(
                "Connected to MCP server {server_name}: {name} v{version}",
                server_name = server_name,
                name = init_result.server_info.name,
                version = init_result.server_info.version
            );

            // Send initialized notification
            debug!("Sending initialized notification to {server_name}");
            let notification = Request::notification("initialized", Some(serde_json::json!({})));
            Self::send_notification(stdin.clone(), notification).await?;
            debug!("Initialized notification sent to {server_name}");

            // Mark server as initialized and healthy
            let mut server_guard = server.lock().await;
            server_guard.initialized = true;
            server_guard.last_healthy = Some(Utc::now());
            server_guard.error_count = 0;
            drop(server_guard);

            // Discover available tools with timeout
            debug!("Starting tool discovery for {server_name}");
            match timeout(
                Duration::from_secs(5),
                Self::discover_server_tools(server.clone(), stdin.clone()),
            )
            .await
            {
                Ok(Ok(())) => {
                    debug!("Tool discovery completed for {server_name}");
                }
                Ok(Err(e)) => {
                    error!("Tool discovery failed for {server_name}: {e}");
                    return Err(e);
                }
                Err(_) => {
                    error!("Tool discovery timed out for {server_name}");
                    bail!("Tool discovery timed out");
                }
            }
        } else if let Some(error) = response.error {
            bail!(
                "Initialize failed: {message} (code: {code})",
                message = error.message,
                code = error.code
            );
        }

        Ok(())
    }

    async fn discover_server_tools(
        server: Arc<Mutex<MCPServer>>,
        stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    ) -> Result<()> {
        let request = Request::new("tools/list", Some(serde_json::json!({})));
        let response = Self::send_request(server.clone(), stdin, request).await?;

        if let Some(result) = response.result {
            let tools_result: ToolsListResult = serde_json::from_value(result)?;
            let mut server_guard = server.lock().await;
            server_guard.tools = tools_result.tools;
            server_guard.tools_cache_time = Some(Utc::now());

            info!(
                "Discovered {count} tools from {name}",
                count = server_guard.tools.len(),
                name = server_guard.name
            );

            for tool in &server_guard.tools {
                debug!(
                    "  - {name}: {description}",
                    name = tool.name,
                    description = tool.description.as_ref().unwrap_or(&"".to_string())
                );
            }
        }

        Ok(())
    }

    async fn send_request(
        server: Arc<Mutex<MCPServer>>,
        stdin: Arc<Mutex<tokio::process::ChildStdin>>,
        request: Request,
    ) -> Result<Response> {
        let request_id = request
            .id
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Request must have an ID"))?;

        // Create oneshot channel for response
        let (tx, rx) = oneshot::channel();

        // Store pending request
        {
            let mut server_guard = server.lock().await;
            server_guard.pending_requests.insert(request_id.clone(), tx);
        }

        // Send request
        let message = Message::Request(request);
        let json = message.to_string()? + "\n";

        {
            let mut stdin_guard = stdin.lock().await;
            stdin_guard.write_all(json.as_bytes()).await?;
            stdin_guard.flush().await?;
        }

        debug!("Sent request: {request}", request = json.trim());

        // Add server name context for debugging
        let server_name = {
            let server_guard = server.lock().await;
            server_guard.name.clone()
        };

        // Wait for response with timeout
        debug!(
            "Waiting for response from {} for request ID: {:?}",
            server_name, request_id
        );
        match timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(response)) => {
                debug!(
                    "Received response from {} for request ID: {:?}",
                    server_name, request_id
                );
                Ok(response)
            }
            Ok(Err(_)) => {
                error!(
                    "Response channel closed for {} request ID: {:?}",
                    server_name, request_id
                );
                bail!("Response channel closed")
            }
            Err(_) => {
                // Clean up pending request on timeout
                error!(
                    "Request timeout for {} request ID: {:?}",
                    server_name, request_id
                );
                let mut server_guard = server.lock().await;
                server_guard.pending_requests.remove(&request_id);
                bail!("Request timeout after 30 seconds")
            }
        }
    }

    async fn send_notification(
        stdin: Arc<Mutex<tokio::process::ChildStdin>>,
        notification: crate::jsonrpc::Notification,
    ) -> Result<()> {
        let message = Message::Notification(notification);
        let json = message.to_string()? + "\n";

        let mut stdin_guard = stdin.lock().await;
        stdin_guard.write_all(json.as_bytes()).await?;
        stdin_guard.flush().await?;

        debug!(
            "Sent notification: {notification}",
            notification = json.trim()
        );
        Ok(())
    }

    async fn handle_message(server: Arc<Mutex<MCPServer>>, message: Message) -> Result<()> {
        match message {
            Message::Response(response) => {
                if let Some(id) = &response.id {
                    let mut server_guard = server.lock().await;
                    if let Some(sender) = server_guard.pending_requests.remove(id) {
                        let _ = sender.send(response);
                    }
                }
            }
            Message::Request(_request) => {
                // MCP servers typically don't send requests to clients
                debug!("Received unexpected request from server");
            }
            Message::Notification(_notification) => {
                // Handle server notifications (e.g., tools/list_changed)
                debug!("Received notification from server");
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    pub async fn list_tools(&self) -> Result<Vec<String>> {
        let mut all_tools = Vec::new();

        for server in &self.servers {
            let server_guard = server.lock().await;
            for tool in &server_guard.tools {
                all_tools.push(format!(
                    "{server}:{tool}",
                    server = server_guard.name,
                    tool = tool.name
                ));
            }
        }

        Ok(all_tools)
    }

    pub async fn discover_tools(&mut self) -> Result<Vec<Tool>> {
        let mut all_tools = Vec::new();

        for server in &self.servers {
            let server_guard = server.lock().await;
            for tool_info in &server_guard.tools {
                all_tools.push(Tool {
                    name: format!(
                        "{server}:{tool}",
                        server = server_guard.name,
                        tool = tool_info.name
                    ),
                    description: tool_info.description.clone(),
                    parameters: tool_info.input_schema.clone(),
                });
            }
        }

        Ok(all_tools)
    }

    pub async fn get_tools_with_schemas(&self) -> Result<Vec<Tool>> {
        let mut all_tools = Vec::new();

        for server in &self.servers {
            let server_guard = server.lock().await;
            for tool_info in &server_guard.tools {
                all_tools.push(Tool {
                    name: format!(
                        "{server}:{tool}",
                        server = server_guard.name,
                        tool = tool_info.name
                    ),
                    description: tool_info.description.clone(),
                    parameters: tool_info.input_schema.clone(),
                });
            }
        }

        Ok(all_tools)
    }

    fn start_health_monitoring(&self) {
        let servers = self.servers.clone();

        tokio::spawn(async move {
            let mut check_interval = interval(Duration::from_secs(60));

            loop {
                check_interval.tick().await;

                for server in &servers {
                    let server_guard = server.lock().await;
                    let server_name = server_guard.name.clone();
                    let should_check = server_guard.initialized;
                    drop(server_guard);

                    if should_check && let Err(e) = Self::health_check(server.clone()).await {
                        warn!("Health check failed for {server_name}: {e}");
                    }
                }
            }
        });
    }

    async fn health_check(server: Arc<Mutex<MCPServer>>) -> Result<()> {
        let stdin = {
            let server_guard = server.lock().await;
            server_guard.stdin.clone()
        };

        if let Some(stdin) = stdin {
            // Send a simple ping request to check if server is responsive
            let request = Request::new("tools/list", Some(serde_json::json!({})));

            match timeout(
                Duration::from_secs(5),
                Self::send_request(server.clone(), stdin, request),
            )
            .await
            {
                Ok(Ok(_response)) => {
                    let mut server_guard = server.lock().await;
                    server_guard.last_healthy = Some(Utc::now());
                    server_guard.error_count = 0;
                    Ok(())
                }
                Ok(Err(e)) => {
                    let mut server_guard = server.lock().await;
                    server_guard.error_count += 1;

                    if server_guard.error_count > 3 {
                        warn!(
                            "Server {} appears unhealthy after {} consecutive failures",
                            server_guard.name, server_guard.error_count
                        );

                        // Try to restart the server
                        drop(server_guard);
                        if let Err(restart_err) = Self::restart_server(server.clone()).await {
                            error!("Failed to restart unhealthy server: {restart_err}");
                        }
                    }

                    Err(e)
                }
                Err(_) => {
                    let mut server_guard = server.lock().await;
                    server_guard.error_count += 1;

                    if server_guard.error_count > 3 {
                        warn!(
                            "Server {} appears unhealthy after {} consecutive failures (timeout)",
                            server_guard.name, server_guard.error_count
                        );

                        // Try to restart the server
                        drop(server_guard);
                        if let Err(e) = Self::restart_server(server.clone()).await {
                            error!("Failed to restart unhealthy server: {e}");
                        }
                    }

                    bail!("Health check timeout")
                }
            }
        } else {
            bail!("No stdin handle for health check")
        }
    }

    async fn restart_server(server: Arc<Mutex<MCPServer>>) -> Result<()> {
        info!("Attempting to restart MCP server");

        // Clean up existing process
        {
            let mut server_guard = server.lock().await;
            if let Some(mut process) = server_guard.process.take() {
                let _ = process.kill().await;
            }
            server_guard.initialized = false;
            server_guard.stdin = None;
            server_guard.tools.clear();
            server_guard.pending_requests.clear();
        }

        // Try to start the server again
        Self::start_server(server).await
    }

    pub async fn get_health_status(&self) -> Vec<MCPServerHealth> {
        let mut health_status = Vec::new();

        for server in &self.servers {
            let server_guard = server.lock().await;
            health_status.push(MCPServerHealth {
                name: server_guard.name.clone(),
                is_healthy: server_guard
                    .last_healthy
                    .is_some_and(|t| (Utc::now() - t).num_seconds() < 120),
                is_initialized: server_guard.initialized,
                last_healthy: server_guard.last_healthy,
                error_count: server_guard.error_count,
                tool_count: server_guard.tools.len(),
            });
        }

        health_status
    }

    pub async fn use_tool(&self, name: &str, params: Value) -> Result<Value> {
        debug!("Using tool: {name} with params: {params:?}");

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
            .find(|s| {
                let server_guard = futures::executor::block_on(s.lock());
                server_guard.name == server_name
            })
            .ok_or_else(|| anyhow::anyhow!("Server not found: {}", server_name))?;

        // Get stdin handle and check if server is initialized
        let stdin = {
            let server_guard = server.lock().await;
            if !server_guard.initialized {
                bail!("Server {} is not initialized", server_name);
            }
            server_guard
                .stdin
                .clone()
                .ok_or_else(|| anyhow::anyhow!("No stdin handle for server {}", server_name))?
        };

        // Create tool call request
        let tool_params = ToolCallParams {
            name: tool_name.to_string(),
            arguments: Some(params),
        };

        let request = Request::new("tools/call", Some(serde_json::to_value(tool_params)?));
        let response = Self::send_request(server.clone(), stdin, request).await?;

        // Parse tool execution response
        if let Some(result) = response.result {
            // Update last healthy timestamp on successful tool use
            {
                let mut server_guard = server.lock().await;
                server_guard.last_healthy = Some(Utc::now());
                server_guard.error_count = 0;
            }

            let tool_result: ToolCallResult = serde_json::from_value(result)?;

            // Convert tool result to appropriate format
            if let Some(content) = tool_result.content
                && !content.is_empty()
            {
                // Extract text content from the first item
                if let Some(ContentItem::Text { text }) = content.into_iter().next() {
                    // Try to parse as JSON to preserve structure
                    if let Ok(json_value) = serde_json::from_str::<Value>(&text) {
                        // Return the parsed JSON directly to preserve all fields
                        return Ok(json_value);
                    }
                    // Fall back to simple format if not valid JSON
                    debug!(
                        "Tool result is not valid JSON, using simplified format. Content preview: {}...",
                        &text.chars().take(100).collect::<String>()
                    );
                    return Ok(serde_json::json!({
                        "success": !tool_result.is_error.unwrap_or(false),
                        "content": text
                    }));
                }
            }

            Ok(serde_json::json!({
                "success": !tool_result.is_error.unwrap_or(false),
                "message": format!("Tool {tool_name} executed")
            }))
        } else if let Some(error) = response.error {
            bail!(
                "Tool execution failed: {message} (code: {code})",
                message = error.message,
                code = error.code
            )
        } else {
            bail!("Invalid tool execution response")
        }
    }
}

impl Drop for MCPClient {
    fn drop(&mut self) {
        // Clean up any running MCP server processes without blocking
        for server in &self.servers {
            // Use try_lock to avoid potential deadlocks and hanging
            if let Ok(mut server_guard) = server.try_lock()
                && let Some(mut process) = server_guard.process.take()
            {
                // Use start_kill() which is non-blocking, let OS handle cleanup
                let _ = process.start_kill();
                debug!(
                    "Initiated shutdown of MCP server: {name}",
                    name = server_guard.name
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::{Message, Request};

    #[test]
    fn test_json_rpc_request_creation() {
        let request = Request::new("test_method", Some(serde_json::json!({"key": "value"})));
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "test_method");
        assert!(request.id.is_some());
    }

    #[test]
    fn test_json_rpc_message_parsing() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"test":"value"}}"#;
        let message = Message::parse(json).unwrap();

        match message {
            Message::Response(response) => {
                assert_eq!(response.jsonrpc, "2.0");
                assert!(response.result.is_some());
            }
            _ => panic!("Expected Response message"),
        }
    }

    #[test]
    fn test_mcp_initialize_params() {
        let params = InitializeParams::new("test-client".to_string(), "1.0.0".to_string());
        assert_eq!(params.protocol_version, "2024-11-05");
        assert_eq!(params.client_info.name, "test-client");
        assert_eq!(params.client_info.version, "1.0.0");
    }

    #[tokio::test]
    async fn test_mcp_client_creation_with_echo() -> Result<()> {
        // Use a simple echo command that immediately exits
        let configs = vec![MCPServerConfig {
            name: "test".to_string(),
            transport: "stdio".to_string(),
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            retry_attempts: 1,
            retry_delay_ms: 100,
            health_check_interval_secs: 60,
        }];

        let client = MCPClient::new(&configs).await?;
        assert_eq!(client.servers.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn test_mcp_client_handles_missing_command() -> Result<()> {
        let configs = vec![MCPServerConfig {
            name: "missing".to_string(),
            transport: "stdio".to_string(),
            command: "nonexistent_command_12345".to_string(),
            args: vec![],
            retry_attempts: 1,
            retry_delay_ms: 100,
            health_check_interval_secs: 60,
        }];

        // Should not panic, just log error
        let client = MCPClient::new(&configs).await?;
        assert_eq!(client.servers.len(), 1);

        // Server should not be initialized
        let server = &client.servers[0];
        let server_guard = server.lock().await;
        assert!(!server_guard.initialized);

        Ok(())
    }

    #[tokio::test]
    async fn test_list_tools_empty() -> Result<()> {
        let configs = vec![];
        let client = MCPClient::new(&configs).await?;
        let tools = client.list_tools().await?;
        assert!(tools.is_empty());
        Ok(())
    }

    #[test]
    fn test_tool_call_params() {
        let params = ToolCallParams {
            name: "test_tool".to_string(),
            arguments: Some(serde_json::json!({"arg": "value"})),
        };

        let json = serde_json::to_value(params).unwrap();
        assert_eq!(json["name"], "test_tool");
        assert_eq!(json["arguments"]["arg"], "value");
    }

    #[test]
    fn test_content_item_text() {
        let item = ContentItem::Text {
            text: "Hello, world!".to_string(),
        };

        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["type"], "text");
        assert_eq!(json["text"], "Hello, world!");
    }
}
