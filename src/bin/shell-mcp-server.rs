#!/usr/bin/env rust
//! Shell Command MCP Server for Fedimint Challenge
//! Provides shell command execution with Docker support

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Stdio as ProcessStdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::runtime::Runtime;
use tokio::time::timeout;

/// Shell MCP Server implementation
struct ShellMCPServer {
    initialized: bool,
    workspace_root: PathBuf,
    allow_docker: bool,
    runtime: Runtime,
}

impl ShellMCPServer {
    fn new() -> Result<Self> {
        let workspace_root =
            std::env::var("WORKSPACE_PATH").unwrap_or_else(|_| "/workspace".to_string());

        let workspace_root = PathBuf::from(workspace_root)
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from("/workspace"));

        let allow_docker = std::env::var("ALLOW_DOCKER")
            .unwrap_or_else(|_| "true".to_string())
            .to_lowercase()
            == "true";

        eprintln!("[Shell MCP] Workspace root: {:?}", workspace_root);
        eprintln!("[Shell MCP] Docker support: {}", allow_docker);

        let runtime = Runtime::new()?;

        Ok(Self {
            initialized: false,
            workspace_root,
            allow_docker,
            runtime,
        })
    }

    /// Ensure working directory is within workspace
    fn safe_cwd(&self, cwd: &str) -> Result<PathBuf> {
        let full_path = if cwd.starts_with('/') {
            PathBuf::from(cwd)
        } else {
            self.workspace_root.join(cwd)
        };

        let canonical = full_path
            .canonicalize()
            .unwrap_or_else(|_| full_path.clone());

        if !canonical.starts_with(&self.workspace_root) {
            bail!("Working directory {} is outside workspace", cwd);
        }

        Ok(canonical)
    }

    /// Handle JSON-RPC request
    fn handle_request(&mut self, request: Value) -> Result<Option<Value>> {
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let default_params = json!({});
        let params = request.get("params").unwrap_or(&default_params);
        let request_id = request.get("id");

        eprintln!("[Shell MCP] Handling request: {}", method);

        match method {
            "initialize" => Ok(Some(self.handle_initialize(request_id)?)),
            "initialized" => {
                self.initialized = true;
                eprintln!("[Shell MCP] Client confirmed initialization");
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
    fn handle_initialize(&self, request_id: Option<&Value>) -> Result<Value> {
        Ok(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "shell-mcp-server",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {}
                }
            }
        }))
    }

    /// Return list of available tools
    fn handle_tools_list(&self, request_id: Option<&Value>) -> Result<Value> {
        let mut tools = vec![
            json!({
                "name": "run_command",
                "description": "Execute a shell command",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "The shell command to execute"},
                        "cwd": {"type": "string", "description": "Working directory", "default": "."},
                        "timeout_secs": {"type": "integer", "description": "Command timeout in seconds", "default": 60}
                    },
                    "required": ["command"]
                }
            }),
            json!({
                "name": "check_command",
                "description": "Check if a command is available",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "Command name to check"}
                    },
                    "required": ["command"]
                }
            }),
        ];

        if self.allow_docker {
            tools.extend(vec![
                json!({
                    "name": "docker_run",
                    "description": "Run a Docker container",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "image": {"type": "string", "description": "Docker image to run"},
                            "command": {"type": "string", "description": "Command to run in container"},
                            "name": {"type": "string", "description": "Container name"},
                            "detach": {"type": "boolean", "description": "Run in background", "default": false},
                            "ports": {"type": "array", "items": {"type": "string"}, "description": "Port mappings"},
                            "volumes": {"type": "array", "items": {"type": "string"}, "description": "Volume mappings"},
                            "env": {"type": "object", "description": "Environment variables"}
                        },
                        "required": ["image"]
                    }
                }),
                json!({
                    "name": "docker_ps",
                    "description": "List Docker containers",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "all": {"type": "boolean", "description": "Show all containers", "default": false}
                        }
                    }
                }),
                json!({
                    "name": "docker_logs",
                    "description": "Get Docker container logs",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "container": {"type": "string", "description": "Container name or ID"},
                            "tail": {"type": "integer", "description": "Number of lines from end", "default": 50}
                        },
                        "required": ["container"]
                    }
                }),
                json!({
                    "name": "docker_exec",
                    "description": "Execute command in running container",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "container": {"type": "string", "description": "Container name or ID"},
                            "command": {"type": "string", "description": "Command to execute"}
                        },
                        "required": ["container", "command"]
                    }
                }),
                json!({
                    "name": "docker_stop",
                    "description": "Stop a Docker container",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "container": {"type": "string", "description": "Container name or ID"}
                        },
                        "required": ["container"]
                    }
                }),
                json!({
                    "name": "docker_pull",
                    "description": "Pull a Docker image",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "image": {"type": "string", "description": "Image to pull"}
                        },
                        "required": ["image"]
                    }
                })
            ]);
        }

        Ok(json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": {
                "tools": tools
            }
        }))
    }

    /// Handle tool execution
    fn handle_tool_call(&mut self, request_id: Option<&Value>, params: &Value) -> Result<Value> {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let default_args = json!({});
        let arguments = params.get("arguments").unwrap_or(&default_args);

        eprintln!("[Shell MCP] Executing tool: {}", tool_name);

        let result = match tool_name {
            "run_command" => self.run_command(arguments),
            "check_command" => self.check_command(arguments),
            "docker_run" if self.allow_docker => self.docker_run(arguments),
            "docker_ps" if self.allow_docker => self.docker_ps(arguments),
            "docker_logs" if self.allow_docker => self.docker_logs(arguments),
            "docker_exec" if self.allow_docker => self.docker_exec(arguments),
            "docker_stop" if self.allow_docker => self.docker_stop(arguments),
            "docker_pull" if self.allow_docker => self.docker_pull(arguments),
            _ => Err(anyhow::anyhow!("Unknown tool: {}", tool_name)),
        };

        match result {
            Ok(content) => Ok(json!({
                "jsonrpc": "2.0",
                "id": request_id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": content
                        }
                    ]
                }
            })),
            Err(e) => Ok(self.error_response(
                request_id,
                -32603,
                &format!("Tool execution failed: {}", e),
            )),
        }
    }

    fn run_command(&mut self, args: &Value) -> Result<String> {
        let command = args
            .get("command")
            .and_then(|c| c.as_str())
            .context("Missing 'command' parameter")?;

        let cwd = args.get("cwd").and_then(|c| c.as_str()).unwrap_or(".");

        let timeout_secs = args
            .get("timeout_secs")
            .and_then(|t| t.as_u64())
            .unwrap_or(60);

        let safe_cwd = self.safe_cwd(cwd)?;

        self.runtime.block_on(async {
            let output = timeout(
                Duration::from_secs(timeout_secs),
                Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .current_dir(&safe_cwd)
                    .stdout(ProcessStdio::piped())
                    .stderr(ProcessStdio::piped())
                    .output(),
            )
            .await
            .context("Command timed out")?
            .context("Failed to execute command")?;

            let mut result = format!("Exit code: {}\n", output.status.code().unwrap_or(-1));

            if !output.stdout.is_empty() {
                result.push_str("STDOUT:\n");
                result.push_str(&String::from_utf8_lossy(&output.stdout));
                result.push('\n');
            }

            if !output.stderr.is_empty() {
                result.push_str("STDERR:\n");
                result.push_str(&String::from_utf8_lossy(&output.stderr));
            }

            Ok(result)
        })
    }

    fn check_command(&mut self, args: &Value) -> Result<String> {
        let command = args
            .get("command")
            .and_then(|c| c.as_str())
            .context("Missing 'command' parameter")?;

        self.runtime.block_on(async {
            let output = Command::new("which").arg(command).output().await?;

            let available = output.status.success();
            let path = if available {
                Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
            } else {
                None
            };

            Ok(serde_json::to_string(&json!({
                "available": available,
                "path": path
            }))?)
        })
    }

    fn docker_run(&mut self, args: &Value) -> Result<String> {
        let image = args
            .get("image")
            .and_then(|i| i.as_str())
            .context("Missing 'image' parameter")?;

        self.runtime.block_on(async {
            let mut cmd = Command::new("docker");
            cmd.arg("run");

            if let Some(name) = args.get("name").and_then(|n| n.as_str()) {
                cmd.arg("--name").arg(name);
            }

            if args
                .get("detach")
                .and_then(|d| d.as_bool())
                .unwrap_or(false)
            {
                cmd.arg("-d");
            }

            if let Some(ports) = args.get("ports").and_then(|p| p.as_array()) {
                for port in ports {
                    if let Some(port_str) = port.as_str() {
                        cmd.arg("-p").arg(port_str);
                    }
                }
            }

            if let Some(volumes) = args.get("volumes").and_then(|v| v.as_array()) {
                for volume in volumes {
                    if let Some(vol_str) = volume.as_str() {
                        // Ensure host paths are within workspace
                        let vol_parts: Vec<&str> = vol_str.split(':').collect();
                        if vol_parts.len() >= 2 && !vol_parts[0].starts_with('/') {
                            let safe_path = self.safe_cwd(vol_parts[0])?;
                            let vol =
                                format!("{}:{}", safe_path.display(), vol_parts[1..].join(":"));
                            cmd.arg("-v").arg(vol);
                        } else {
                            cmd.arg("-v").arg(vol_str);
                        }
                    }
                }
            }

            if let Some(env_obj) = args.get("env").and_then(|e| e.as_object()) {
                for (key, value) in env_obj {
                    if let Some(val_str) = value.as_str() {
                        cmd.arg("-e").arg(format!("{}={}", key, val_str));
                    }
                }
            }

            cmd.arg(image);

            if let Some(command) = args.get("command").and_then(|c| c.as_str()) {
                cmd.args(command.split_whitespace());
            }

            let output = timeout(Duration::from_secs(120), cmd.output()).await??;

            let mut result = format!("Exit code: {}\n", output.status.code().unwrap_or(-1));
            if !output.stdout.is_empty() {
                result.push_str("Output:\n");
                result.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                result.push_str("Errors:\n");
                result.push_str(&String::from_utf8_lossy(&output.stderr));
            }

            Ok(result)
        })
    }

    fn docker_ps(&mut self, args: &Value) -> Result<String> {
        self.runtime.block_on(async {
            let mut cmd = Command::new("docker");
            cmd.arg("ps").arg("--format").arg("json");

            if args.get("all").and_then(|a| a.as_bool()).unwrap_or(false) {
                cmd.arg("-a");
            }

            let output = cmd.output().await?;

            let mut containers = Vec::new();
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if !line.trim().is_empty()
                    && let Ok(container) = serde_json::from_str::<Value>(line)
                {
                    containers.push(container);
                }
            }

            Ok(serde_json::to_string_pretty(&json!(containers))?)
        })
    }

    fn docker_logs(&mut self, args: &Value) -> Result<String> {
        let container = args
            .get("container")
            .and_then(|c| c.as_str())
            .context("Missing 'container' parameter")?;

        self.runtime.block_on(async {
            let mut cmd = Command::new("docker");
            cmd.arg("logs");

            if let Some(tail) = args.get("tail").and_then(|t| t.as_u64()) {
                cmd.arg("--tail").arg(tail.to_string());
            }

            cmd.arg(container);

            let output = timeout(Duration::from_secs(30), cmd.output()).await??;

            let mut result = String::new();
            if !output.stdout.is_empty() {
                result.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                result.push_str(&String::from_utf8_lossy(&output.stderr));
            }

            Ok(result)
        })
    }

    fn docker_exec(&mut self, args: &Value) -> Result<String> {
        let container = args
            .get("container")
            .and_then(|c| c.as_str())
            .context("Missing 'container' parameter")?;

        let command = args
            .get("command")
            .and_then(|c| c.as_str())
            .context("Missing 'command' parameter")?;

        self.runtime.block_on(async {
            let mut cmd = Command::new("docker");
            cmd.arg("exec").arg(container);
            cmd.args(command.split_whitespace());

            let output = timeout(Duration::from_secs(60), cmd.output()).await??;

            let mut result = format!("Exit code: {}\n", output.status.code().unwrap_or(-1));
            if !output.stdout.is_empty() {
                result.push_str("Output:\n");
                result.push_str(&String::from_utf8_lossy(&output.stdout));
            }
            if !output.stderr.is_empty() {
                result.push_str("Errors:\n");
                result.push_str(&String::from_utf8_lossy(&output.stderr));
            }

            Ok(result)
        })
    }

    fn docker_stop(&mut self, args: &Value) -> Result<String> {
        let container = args
            .get("container")
            .and_then(|c| c.as_str())
            .context("Missing 'container' parameter")?;

        self.runtime.block_on(async {
            let output = timeout(
                Duration::from_secs(30),
                Command::new("docker").arg("stop").arg(container).output(),
            )
            .await??;

            if output.status.success() {
                Ok(format!("Container {} stopped", container))
            } else {
                Err(anyhow::anyhow!(
                    "Failed to stop container: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        })
    }

    fn docker_pull(&mut self, args: &Value) -> Result<String> {
        let image = args
            .get("image")
            .and_then(|i| i.as_str())
            .context("Missing 'image' parameter")?;

        self.runtime.block_on(async {
            let output = timeout(
                Duration::from_secs(300),
                Command::new("docker").arg("pull").arg(image).output(),
            )
            .await??;

            if output.status.success() {
                Ok(format!("Successfully pulled {}", image))
            } else {
                Err(anyhow::anyhow!(
                    "Failed to pull image: {}",
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        })
    }

    /// Create error response
    fn error_response(&self, request_id: Option<&Value>, code: i64, message: &str) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "error": {
                "code": code,
                "message": message
            }
        })
    }
}

fn main() -> Result<()> {
    eprintln!("[Shell MCP] Starting server...");

    let mut server = ShellMCPServer::new()?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());

    eprintln!("[Shell MCP] Ready for requests");

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        eprintln!("[Shell MCP] Received: {}", line);

        let request: Value = serde_json::from_str(&line)
            .with_context(|| format!("Failed to parse JSON: {}", line))?;

        match server.handle_request(request) {
            Ok(Some(response)) => {
                let response_str = serde_json::to_string(&response)?;
                writeln!(stdout, "{}", response_str)?;
                stdout.flush()?;
                eprintln!("[Shell MCP] Sent response: {}", response_str);
            }
            Ok(None) => {
                eprintln!("[Shell MCP] No response needed");
            }
            Err(e) => {
                eprintln!("[Shell MCP] Error handling request: {}", e);
                let error_response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32603,
                        "message": format!("Internal error: {}", e)
                    }
                });
                writeln!(stdout, "{}", serde_json::to_string(&error_response)?)?;
                stdout.flush()?;
            }
        }
    }

    eprintln!("[Shell MCP] Server shutting down");
    Ok(())
}
