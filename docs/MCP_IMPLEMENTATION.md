# MCP (Model Context Protocol) Implementation

## Overview

Replicante now includes a fully functional MCP client that can communicate with real MCP servers using JSON-RPC 2.0 over stdio transport.

## Architecture

### Components

1. **JSON-RPC Layer** (`src/jsonrpc.rs`)
   - Implements JSON-RPC 2.0 message types
   - Handles request/response correlation
   - Manages message serialization/deserialization

2. **MCP Protocol** (`src/mcp_protocol.rs`)
   - Defines MCP-specific message structures
   - Implements protocol version negotiation
   - Handles capability discovery

3. **MCP Client** (`src/mcp.rs`)
   - Manages MCP server processes
   - Handles stdio communication
   - Implements tool discovery and invocation
   - Manages server lifecycle

## How It Works

### Connection Flow

1. **Server Startup**
   - Spawn MCP server process with piped stdio
   - Create async tasks for stdout/stderr handling
   - Store stdin handle for sending requests

2. **Protocol Handshake**
   ```json
   → {"jsonrpc":"2.0","method":"initialize","params":{...},"id":1}
   ← {"jsonrpc":"2.0","result":{...},"id":1}
   → {"jsonrpc":"2.0","method":"initialized","params":{}}
   ```

3. **Tool Discovery**
   ```json
   → {"jsonrpc":"2.0","method":"tools/list","params":{},"id":2}
   ← {"jsonrpc":"2.0","result":{"tools":[...]},"id":2}
   ```

4. **Tool Invocation**
   ```json
   → {"jsonrpc":"2.0","method":"tools/call","params":{...},"id":3}
   ← {"jsonrpc":"2.0","result":{...},"id":3}
   ```

### Key Features

- **Async Process Management**: Uses Tokio for non-blocking I/O
- **Request Correlation**: Tracks pending requests with unique IDs
- **Error Handling**: Timeouts, retries, and graceful degradation
- **Clean Shutdown**: Properly terminates server processes on exit
- **Tool Namespacing**: Tools are prefixed with server name (e.g., `filesystem:fs_read`)

## Configuration

Add MCP servers to your `config.toml`:

```toml
[[mcp_servers]]
name = "filesystem"
transport = "stdio"
command = "mcp-server-filesystem"
args = ["--root", "/data"]

[[mcp_servers]]
name = "http"
transport = "stdio"
command = "mcp-server-http"
args = []
```

## Security Considerations

1. **Process Isolation**: Each MCP server runs as a separate process
2. **Sandboxing**: Servers can implement their own sandboxing (e.g., filesystem `--root`)
3. **No Direct File Access**: The agent only accesses files through MCP servers
4. **Audit Logging**: All tool invocations are logged

## Testing

The implementation can be tested with mock MCP servers:

```bash
# Create a test config with echo server
cat > test_config.toml << EOF
[[mcp_servers]]
name = "test"
transport = "stdio"
command = "echo"
args = ["test response"]
EOF

# Run with test config
cargo run -- --config test_config.toml
```

## Compatibility

- **Protocol Version**: 2024-11-05
- **Transport**: stdio (stdin/stdout)
- **Message Format**: JSON-RPC 2.0 (newline-delimited)
- **Capabilities**: Tools (resources and prompts planned)

## Future Enhancements

- [ ] Add WebSocket transport support
- [ ] Implement resource management
- [ ] Add prompt templates
- [ ] Support server-sent notifications
- [ ] Add connection retry logic
- [ ] Implement server health monitoring