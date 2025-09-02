#!/usr/bin/env rust
//! Filesystem MCP Server for Fedimint Challenge
//! Provides file operations within a sandboxed workspace

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::runtime::Runtime;

/// Command-line arguments for the filesystem MCP server
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Workspace root directory for sandboxed operations
    #[arg(long, env = "WORKSPACE_PATH", default_value = "/workspace")]
    workspace: PathBuf,

    /// Enable verbose output
    #[arg(long, env = "MCP_VERBOSE")]
    verbose: bool,
}

/// Filesystem MCP Server implementation
struct FilesystemMCPServer {
    initialized: bool,
    workspace_root: PathBuf,
    runtime: Runtime,
    verbose: bool,
}

impl FilesystemMCPServer {
    fn new(args: Args) -> Result<Self> {
        let workspace_root = args
            .workspace
            .canonicalize()
            .unwrap_or_else(|_| args.workspace.clone());

        if args.verbose {
            eprintln!("[Filesystem MCP] Workspace root: {:?}", workspace_root);
        }

        let runtime = Runtime::new()?;

        Ok(Self {
            initialized: false,
            workspace_root,
            runtime,
            verbose: args.verbose,
        })
    }

    /// Ensure path is within workspace
    fn safe_path(&self, path: &str) -> Result<PathBuf> {
        let full_path = if path.starts_with('/') {
            PathBuf::from(path)
        } else {
            self.workspace_root.join(path)
        };

        let canonical = full_path.canonicalize().or_else(|_| {
            // If file doesn't exist yet, canonicalize parent
            if let Some(parent) = full_path.parent() {
                parent
                    .canonicalize()
                    .map(|p| p.join(full_path.file_name().unwrap_or_default()))
            } else {
                Ok(full_path.clone())
            }
        })?;

        // Check if path is within workspace
        if !canonical.starts_with(&self.workspace_root) {
            bail!("Path {} is outside workspace", path);
        }

        Ok(canonical)
    }

    /// Handle JSON-RPC request
    fn handle_request(&mut self, request: Value) -> Result<Option<Value>> {
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let default_params = json!({});
        let params = request.get("params").unwrap_or(&default_params);
        let request_id = request.get("id");

        if self.verbose {
            eprintln!("[Filesystem MCP] Handling request: {}", method);
        }

        match method {
            "initialize" => Ok(Some(self.handle_initialize(request_id)?)),
            "initialized" => {
                self.initialized = true;
                if self.verbose {
                    eprintln!("[Filesystem MCP] Client confirmed initialization");
                }
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
                    "name": "filesystem-mcp-server",
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
        let tools = json!([
            {
                "name": "read_file",
                "description": "Read the contents of a file",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Path to the file relative to workspace"}
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "write_file",
                "description": "Write content to a file",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Path to the file relative to workspace"},
                        "content": {"type": "string", "description": "Content to write"},
                        "append": {"type": "boolean", "description": "Append instead of overwrite", "default": false}
                    },
                    "required": ["path", "content"]
                }
            },
            {
                "name": "list_directory",
                "description": "List files and directories",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path", "default": "."},
                        "recursive": {"type": "boolean", "description": "List recursively", "default": false}
                    }
                }
            },
            {
                "name": "create_directory",
                "description": "Create a directory",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Directory path"}
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "delete_file",
                "description": "Delete a file or empty directory",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Path to delete"}
                    },
                    "required": ["path"]
                }
            },
            {
                "name": "file_exists",
                "description": "Check if a file or directory exists",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Path to check"}
                    },
                    "required": ["path"]
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

    /// Handle tool execution
    fn handle_tool_call(&mut self, request_id: Option<&Value>, params: &Value) -> Result<Value> {
        let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let default_args = json!({});
        let arguments = params.get("arguments").unwrap_or(&default_args);

        if self.verbose {
            eprintln!("[Filesystem MCP] Executing tool: {}", tool_name);
        }

        let result = match tool_name {
            "read_file" => self.read_file(arguments),
            "write_file" => self.write_file(arguments),
            "list_directory" => self.list_directory(arguments),
            "create_directory" => self.create_directory(arguments),
            "delete_file" => self.delete_file(arguments),
            "file_exists" => self.file_exists(arguments),
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

    fn read_file(&mut self, args: &Value) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .context("Missing 'path' parameter")?;

        let safe_path = self.safe_path(path)?;

        self.runtime.block_on(async {
            let content = fs::read_to_string(&safe_path)
                .await
                .with_context(|| format!("Failed to read file: {:?}", safe_path))?;
            Ok(content)
        })
    }

    fn write_file(&mut self, args: &Value) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .context("Missing 'path' parameter")?;

        let content = args
            .get("content")
            .and_then(|c| c.as_str())
            .context("Missing 'content' parameter")?;

        let append = args
            .get("append")
            .and_then(|a| a.as_bool())
            .unwrap_or(false);

        let safe_path = self.safe_path(path)?;

        self.runtime.block_on(async {
            // Create parent directories if needed
            if let Some(parent) = safe_path.parent() {
                fs::create_dir_all(parent).await?;
            }

            if append {
                use tokio::io::AsyncWriteExt;
                let mut file = fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&safe_path)
                    .await?;
                file.write_all(content.as_bytes()).await?;
            } else {
                fs::write(&safe_path, content).await?;
            }

            Ok(format!("Successfully wrote to {}", path))
        })
    }

    fn list_directory(&mut self, args: &Value) -> Result<String> {
        let path = args.get("path").and_then(|p| p.as_str()).unwrap_or(".");

        let recursive = args
            .get("recursive")
            .and_then(|r| r.as_bool())
            .unwrap_or(false);

        let safe_path = self.safe_path(path)?;

        self.runtime.block_on(async {
            let mut entries = Vec::new();

            if recursive {
                self.list_recursive(&safe_path, &self.workspace_root, &mut entries)
                    .await?;
            } else {
                let mut dir = fs::read_dir(&safe_path).await?;
                while let Some(entry) = dir.next_entry().await? {
                    let path = entry.path();
                    let rel_path = path
                        .strip_prefix(&self.workspace_root)
                        .unwrap_or(&path)
                        .to_string_lossy();

                    let metadata = entry.metadata().await?;
                    let prefix = if metadata.is_dir() { "[DIR] " } else { "" };
                    entries.push(format!("{}{}", prefix, rel_path));
                }
            }

            entries.sort();
            Ok(if entries.is_empty() {
                "Empty directory".to_string()
            } else {
                entries.join("\n")
            })
        })
    }

    #[allow(clippy::only_used_in_recursion)]
    fn list_recursive<'a>(
        &'a self,
        dir: &'a Path,
        base: &'a Path,
        entries: &'a mut Vec<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + 'a>> {
        Box::pin(async move {
            let mut dir_entries = fs::read_dir(dir).await?;
            while let Some(entry) = dir_entries.next_entry().await? {
                let path = entry.path();
                let rel_path = path.strip_prefix(base).unwrap_or(&path).to_string_lossy();

                let metadata = entry.metadata().await?;
                if metadata.is_dir() {
                    entries.push(format!("[DIR] {}", rel_path));
                    self.list_recursive(&path, base, entries).await?;
                } else {
                    entries.push(rel_path.to_string());
                }
            }
            Ok(())
        })
    }

    fn create_directory(&mut self, args: &Value) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .context("Missing 'path' parameter")?;

        let safe_path = self.safe_path(path)?;

        self.runtime.block_on(async {
            fs::create_dir_all(&safe_path).await?;
            Ok(format!("Created directory: {}", path))
        })
    }

    fn delete_file(&mut self, args: &Value) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .context("Missing 'path' parameter")?;

        let safe_path = self.safe_path(path)?;

        self.runtime.block_on(async {
            let metadata = fs::metadata(&safe_path).await?;

            if metadata.is_dir() {
                fs::remove_dir(&safe_path)
                    .await
                    .with_context(|| format!("Failed to delete directory: {:?}", safe_path))?;
            } else {
                fs::remove_file(&safe_path)
                    .await
                    .with_context(|| format!("Failed to delete file: {:?}", safe_path))?;
            }

            Ok(format!("Deleted: {}", path))
        })
    }

    fn file_exists(&mut self, args: &Value) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|p| p.as_str())
            .context("Missing 'path' parameter")?;

        let safe_path = self.safe_path(path)?;

        self.runtime.block_on(async {
            let exists = fs::try_exists(&safe_path).await.unwrap_or(false);

            let result = if exists {
                let metadata = fs::metadata(&safe_path).await?;
                json!({
                    "exists": true,
                    "type": if metadata.is_dir() { "directory" } else { "file" },
                    "size": if metadata.is_file() { Some(metadata.len()) } else { None }
                })
            } else {
                json!({
                    "exists": false,
                    "type": null,
                    "size": null
                })
            };

            Ok(serde_json::to_string(&result)?)
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
    let args = Args::parse();
    let verbose = args.verbose;

    if verbose {
        eprintln!("[Filesystem MCP] Starting server...");
    }

    let mut server = FilesystemMCPServer::new(args)?;
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());

    if verbose {
        eprintln!("[Filesystem MCP] Ready for requests");
    }

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if verbose {
            eprintln!("[Filesystem MCP] Received: {}", line);
        }

        let request: Value = serde_json::from_str(&line)
            .with_context(|| format!("Failed to parse JSON: {}", line))?;

        match server.handle_request(request) {
            Ok(Some(response)) => {
                let response_str = serde_json::to_string(&response)?;
                writeln!(stdout, "{}", response_str)?;
                stdout.flush()?;
                if verbose {
                    eprintln!("[Filesystem MCP] Sent response: {}", response_str);
                }
            }
            Ok(None) => {
                if verbose {
                    eprintln!("[Filesystem MCP] No response needed");
                }
            }
            Err(e) => {
                eprintln!("[Filesystem MCP] Error handling request: {}", e);
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

    eprintln!("[Filesystem MCP] Server shutting down");
    Ok(())
}
