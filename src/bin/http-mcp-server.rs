#!/usr/bin/env rust
//! HTTP MCP Server for testing - provides tools for web requests
//!
//! Provides tools: fetch_url, check_weather, get_time, calculate

use anyhow::{Context, Result};
use chrono::{FixedOffset, Utc};
use rand::Rng;
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};
use std::time::Duration;

/// HTTP MCP Server implementation
struct HttpMCPServer {
    initialized: bool,
    client: reqwest::Client,
}

impl HttpMCPServer {
    fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("MCP-Test/1.0")
            .build()
            .expect("Failed to create HTTP client");

        Self {
            initialized: false,
            client,
        }
    }

    /// Handle JSON-RPC request
    fn handle_request(&mut self, request: Value) -> Result<Option<Value>> {
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let default_params = json!({});
        let params = request.get("params").unwrap_or(&default_params);

        let request_id = request.get("id");

        eprintln!("[HTTP MCP] Handling request: {}", method);

        match method {
            "initialize" => Ok(Some(self.handle_initialize(request_id, params)?)),
            "initialized" => {
                // Notification, no response needed
                self.initialized = true;
                eprintln!("[HTTP MCP] Client confirmed initialization");
                Ok(None)
            }
            "tools/list" => Ok(Some(self.handle_tools_list(request_id)?)),
            "tools/call" => Ok(Some(self.handle_tool_call(request_id, params)?)),
            _ => Ok(Some(self.error_response(
                request_id,
                -32601,
                &format!("Method not found: {}", method),
            ))),
        }
    }

    /// Handle initialize request
    fn handle_initialize(&self, request_id: Option<&Value>, params: &Value) -> Result<Value> {
        let default_client_info = json!({});
        let client_info = params.get("clientInfo").unwrap_or(&default_client_info);
        eprintln!("[HTTP MCP] Initialize request from client: {}", client_info);

        Ok(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "http-mcp-server",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {
                        "listChanged": true
                    }
                }
            }
        }))
    }

    /// Return list of available tools
    fn handle_tools_list(&self, request_id: Option<&Value>) -> Result<Value> {
        let tools = json!([
            {
                "name": "fetch_url",
                "description": "Fetch content from a URL",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "The URL to fetch"}
                    },
                    "required": ["url"]
                }
            },
            {
                "name": "check_weather",
                "description": "Get current weather (mock data)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": {"type": "string", "description": "City name"}
                    },
                    "required": ["city"]
                }
            },
            {
                "name": "get_time",
                "description": "Get current time in various timezones",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "timezone": {"type": "string", "description": "Timezone (e.g., UTC, EST, PST)"}
                    }
                }
            },
            {
                "name": "calculate",
                "description": "Perform basic calculations",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "expression": {"type": "string", "description": "Math expression to evaluate"}
                    },
                    "required": ["expression"]
                }
            }
        ]);

        Ok(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "tools": tools
            }
        }))
    }

    /// Execute a tool and return the result
    fn handle_tool_call(&self, request_id: Option<&Value>, params: &Value) -> Result<Value> {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");

        let default_arguments = json!({});
        let arguments = params.get("arguments").unwrap_or(&default_arguments);

        eprintln!(
            "[HTTP MCP] Tool call: {} with args: {}",
            tool_name, arguments
        );

        let result = match tool_name {
            "fetch_url" => self.fetch_url(arguments)?,
            "check_weather" => self.check_weather(arguments)?,
            "get_time" => self.get_time(arguments)?,
            "calculate" => self.calculate(arguments)?,
            _ => {
                return Ok(self.error_response(
                    request_id,
                    -32602,
                    &format!("Unknown tool: {}", tool_name),
                ));
            }
        };

        Ok(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": result
        }))
    }

    /// Fetch content from a URL
    fn fetch_url(&self, args: &Value) -> Result<Value> {
        let url = args.get("url").and_then(|u| u.as_str()).unwrap_or("");

        // Only fetch URLs from safe domains for testing
        let safe_domains = [
            "httpbin.org",
            "jsonplaceholder.typicode.com",
            "api.github.com",
        ];

        if let Ok(parsed_url) = url::Url::parse(url) {
            let host = parsed_url.host_str().unwrap_or("");

            if !safe_domains.iter().any(|&domain| host.contains(domain)) {
                return Ok(json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Error: URL domain '{}' not in safe list for testing", host)
                    }],
                    "isError": true
                }));
            }
        } else {
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": "Error: Invalid URL format"
                }],
                "isError": true
            }));
        }

        // Create runtime for async request
        let rt = tokio::runtime::Runtime::new().context("Failed to create tokio runtime")?;

        let result = rt.block_on(async {
            match self.client.get(url).send().await {
                Ok(response) => {
                    let status = response.status();
                    match response.text().await {
                        Ok(content) => {
                            let truncated_content = if content.len() > 500 {
                                format!("{}...", &content[..500])
                            } else {
                                content
                            };

                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": format!("Status: {}\nContent (first 500 chars):\n{}", status, truncated_content)
                                }],
                                "isError": false
                            })
                        },
                        Err(e) => {
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": format!("Error reading response body: {}", e)
                                }],
                                "isError": true
                            })
                        }
                    }
                },
                Err(e) => {
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Error fetching URL: {}", e)
                        }],
                        "isError": true
                    })
                }
            }
        });

        Ok(result)
    }

    /// Return mock weather data
    fn check_weather(&self, args: &Value) -> Result<Value> {
        let city = args
            .get("city")
            .and_then(|c| c.as_str())
            .unwrap_or("Unknown");

        // Mock weather data
        let mut rng = rand::thread_rng();
        let temp = rng.gen_range(10..=30);
        let conditions = ["Sunny", "Cloudy", "Rainy", "Partly Cloudy"];
        let condition = conditions[rng.gen_range(0..conditions.len())];

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Weather in {}: {}°C, {}", city, temp, condition)
            }],
            "isError": false
        }))
    }

    /// Get current time in specified timezone
    fn get_time(&self, args: &Value) -> Result<Value> {
        let timezone = args
            .get("timezone")
            .and_then(|tz| tz.as_str())
            .unwrap_or("UTC");

        let now = Utc::now();

        // Simple timezone handling
        let time_str = match timezone.to_uppercase().as_str() {
            "UTC" => now.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            "EST" => {
                let offset = FixedOffset::west_opt(5 * 3600).unwrap();
                let local_time = now.with_timezone(&offset);
                local_time.format("%Y-%m-%d %H:%M:%S EST").to_string()
            }
            "PST" => {
                let offset = FixedOffset::west_opt(8 * 3600).unwrap();
                let local_time = now.with_timezone(&offset);
                local_time.format("%Y-%m-%d %H:%M:%S PST").to_string()
            }
            "CET" => {
                let offset = FixedOffset::east_opt(3600).unwrap();
                let local_time = now.with_timezone(&offset);
                local_time.format("%Y-%m-%d %H:%M:%S CET").to_string()
            }
            "JST" => {
                let offset = FixedOffset::east_opt(9 * 3600).unwrap();
                let local_time = now.with_timezone(&offset);
                local_time.format("%Y-%m-%d %H:%M:%S JST").to_string()
            }
            _ => {
                format!(
                    "Current time in {}: {} (timezone not recognized, showing UTC)",
                    timezone,
                    now.format("%Y-%m-%d %H:%M:%S UTC")
                )
            }
        };

        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!("Current time in {}: {}", timezone, time_str)
            }],
            "isError": false
        }))
    }

    /// Evaluate a math expression
    fn calculate(&self, args: &Value) -> Result<Value> {
        let expression = args
            .get("expression")
            .and_then(|e| e.as_str())
            .unwrap_or("");

        // Basic calculator using evalexpr crate for safety
        match evalexpr::eval(expression) {
            Ok(result) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("{} = {}", expression, result)
                }],
                "isError": false
            })),
            Err(e) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Error evaluating expression: {}", e)
                }],
                "isError": true
            })),
        }
    }

    /// Create an error response
    fn error_response(&self, request_id: Option<&Value>, code: i32, message: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": code,
                "message": message
            }
        })
    }

    /// Main server loop
    fn run(&mut self) -> Result<()> {
        eprintln!("[HTTP MCP] HTTP MCP server started");

        let stdin = io::stdin();
        let reader = BufReader::new(stdin.lock());

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(line) {
                Ok(request) => {
                    eprintln!("[HTTP MCP] Received: {}", line);

                    match self.handle_request(request) {
                        Ok(Some(response)) => {
                            let response_json = serde_json::to_string(&response)?;
                            println!("{}", response_json);
                            io::stdout().flush()?;
                            eprintln!("[HTTP MCP] Sent: {}", response_json);
                        }
                        Ok(None) => {
                            // Notification, no response needed
                        }
                        Err(e) => {
                            eprintln!("[HTTP MCP] Error handling request: {}", e);
                            let error = self.error_response(None, -32603, &e.to_string());
                            let error_json = serde_json::to_string(&error)?;
                            println!("{}", error_json);
                            io::stdout().flush()?;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[HTTP MCP] Failed to parse JSON: {}", e);
                    let error = self.error_response(None, -32700, "Parse error");
                    let error_json = serde_json::to_string(&error)?;
                    println!("{}", error_json);
                    io::stdout().flush()?;
                }
            }
        }

        Ok(())
    }
}

fn main() -> Result<()> {
    let mut server = HttpMCPServer::new();

    if let Err(e) = server.run() {
        eprintln!("[HTTP MCP] Server error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
