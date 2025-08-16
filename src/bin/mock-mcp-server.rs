#!/usr/bin/env rust
//! Mock MCP Server for testing Replicante MCP client implementation.
//! Implements a simple MCP server that responds to JSON-RPC requests via stdio.
//!
//! Provides basic tools: echo, add, and get_time

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};

/// Mock MCP Server implementation
struct MockMCPServer {
    initialized: bool,
}

impl MockMCPServer {
    fn new() -> Self {
        Self { initialized: false }
    }

    /// Handle a JSON-RPC request and return a response
    fn handle_request(&mut self, request: Value) -> Result<Option<Value>> {
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let default_params = json!({});
        let params = request.get("params").unwrap_or(&default_params);

        let request_id = request.get("id");

        eprintln!("[Mock MCP] Handling request: {}", method);

        match method {
            "initialize" => Ok(Some(self.handle_initialize(request_id, params)?)),
            "initialized" => {
                // This is a notification, no response needed
                self.initialized = true;
                eprintln!("[Mock MCP] Server initialized");
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

    /// Handle the initialize request
    fn handle_initialize(&self, request_id: Option<&Value>, params: &Value) -> Result<Value> {
        let default_client_info = json!({});
        let client_info = params.get("clientInfo").unwrap_or(&default_client_info);
        eprintln!("[Mock MCP] Initialize request from client: {}", client_info);

        Ok(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "mock-mcp-server",
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

    /// Handle the tools/list request
    fn handle_tools_list(&self, request_id: Option<&Value>) -> Result<Value> {
        eprintln!("[Mock MCP] Listing available tools");

        let tools = json!([
            {
                "name": "echo",
                "description": "Echoes back the input",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"}
                    },
                    "required": ["message"]
                }
            },
            {
                "name": "add",
                "description": "Adds two numbers",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "a": {"type": "number"},
                        "b": {"type": "number"}
                    },
                    "required": ["a", "b"]
                }
            },
            {
                "name": "get_time",
                "description": "Gets the current time",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
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

    /// Handle a tool call request
    fn handle_tool_call(&self, request_id: Option<&Value>, params: &Value) -> Result<Value> {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");

        let default_arguments = json!({});
        let arguments = params.get("arguments").unwrap_or(&default_arguments);

        eprintln!(
            "[Mock MCP] Tool call: {} with args: {}",
            tool_name, arguments
        );

        let result = match tool_name {
            "echo" => {
                let message = arguments
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("");

                json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Echo: {}", message)
                    }],
                    "isError": false
                })
            }
            "add" => {
                let a = arguments.get("a").and_then(|n| n.as_f64()).unwrap_or(0.0);
                let b = arguments.get("b").and_then(|n| n.as_f64()).unwrap_or(0.0);

                json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Result: {}", a + b)
                    }],
                    "isError": false
                })
            }
            "get_time" => {
                let now: DateTime<Utc> = Utc::now();

                json!({
                    "content": [{
                        "type": "text",
                        "text": format!("Current time: {}", now.to_rfc3339())
                    }],
                    "isError": false
                })
            }
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

    /// Main server loop - read from stdin, write to stdout
    fn run(&mut self) -> Result<()> {
        eprintln!(
            "[Mock MCP] Mock MCP server started, PID: {}",
            std::process::id()
        );

        let stdin = io::stdin();
        let reader = BufReader::new(stdin.lock());

        eprintln!("[Mock MCP] Starting main loop, waiting for input...");
        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<Value>(line) {
                Ok(request) => {
                    eprintln!("[Mock MCP] Received: {}", line);

                    match self.handle_request(request) {
                        Ok(Some(response)) => {
                            let response_json = serde_json::to_string(&response)?;
                            println!("{}", response_json);
                            io::stdout().flush()?;
                            eprintln!("[Mock MCP] Sent: {}", response_json);
                        }
                        Ok(None) => {
                            // Notification, no response needed
                        }
                        Err(e) => {
                            eprintln!("[Mock MCP] Error handling request: {}", e);
                            let error = self.error_response(None, -32603, &e.to_string());
                            let error_json = serde_json::to_string(&error)?;
                            println!("{}", error_json);
                            io::stdout().flush()?;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[Mock MCP] Failed to parse JSON: {}", e);
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
    // Set stdout to line buffering for better subprocess communication
    use std::io::Write;
    std::io::stdout().flush().unwrap();

    let mut server = MockMCPServer::new();

    if let Err(e) = server.run() {
        eprintln!("[Mock MCP] Server error: {}", e);
        std::process::exit(1);
    }

    Ok(())
}
