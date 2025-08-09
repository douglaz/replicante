# Deployment Guide

## Building

### Static Binary with Nix

```bash
# Build static musl binary
nix build .#replicante-static

# Result will be at ./result/bin/replicante
ls -lh result/bin/replicante
```

### Manual Build

```bash
# Add musl target
rustup target add x86_64-unknown-linux-musl

# Build static binary
cargo build --release --target x86_64-unknown-linux-musl
```

## Deployment Options

### 1. Simple VPS Deployment

```bash
# Copy binary to server
scp result/bin/replicante user@server:/usr/local/bin/

# Create data directory
ssh user@server 'mkdir -p /data/replicante'

# Create config file on server
scp config.toml user@server:/data/replicante/

# Run with systemd
ssh user@server 'sudo tee /etc/systemd/system/replicante.service' << EOF
[Unit]
Description=Replicante Autonomous Agent
After=network.target

[Service]
Type=simple
User=replicante
WorkingDirectory=/data/replicante
Environment="ANTHROPIC_API_KEY=sk-..."
ExecStart=/usr/local/bin/replicante
Restart=always

[Install]
WantedBy=multi-user.target
EOF

# Start service
ssh user@server 'sudo systemctl enable --now replicante'
```

### 2. Docker Deployment

```dockerfile
# Dockerfile
FROM scratch
COPY replicante /
COPY config.toml /
ENTRYPOINT ["/replicante"]
```

```bash
# Build and run
docker build -t replicante .
docker run -e ANTHROPIC_API_KEY=sk-... replicante
```

### 3. Multiple Providers

Configure different LLM providers:

```bash
# Anthropic Claude
ANTHROPIC_API_KEY=sk-... ./replicante

# OpenAI GPT
OPENAI_API_KEY=sk-... LLM_PROVIDER=openai LLM_MODEL=gpt-4 ./replicante

# Local Ollama
OLLAMA_HOST=http://localhost:11434 LLM_PROVIDER=ollama LLM_MODEL=llama2 ./replicante
```

## MCP Server Setup

### Installing MCP Servers

```bash
# Nostr MCP Server
npm install -g @modelcontextprotocol/server-nostr

# Filesystem MCP Server  
npm install -g @modelcontextprotocol/server-filesystem

# HTTP MCP Server
npm install -g @modelcontextprotocol/server-http
```

### Custom MCP Servers

Add to `config.toml`:

```toml
[[mcp_servers]]
name = "custom"
transport = "stdio"
command = "/path/to/mcp-server"
args = ["--config", "/path/to/config"]
```

## Monitoring

### View Logs

```bash
# If using systemd
journalctl -u replicante -f

# If running directly
RUST_LOG=info ./replicante
```

### Database Inspection

```bash
# View agent memory
sqlite3 replicante.db "SELECT * FROM memory;"

# View recent decisions
sqlite3 replicante.db "SELECT * FROM decisions ORDER BY created_at DESC LIMIT 10;"

# View discovered capabilities
sqlite3 replicante.db "SELECT * FROM capabilities;"
```

## Security Considerations

1. **API Keys**: Use environment variables, never commit to git
2. **Database**: Regularly backup `replicante.db`
3. **Network**: Use firewall rules to restrict access
4. **Resources**: Set resource limits to prevent runaway costs

```bash
# Example resource limits with systemd
[Service]
CPUQuota=50%
MemoryMax=1G
```

## Scaling

### Running Multiple Instances

```bash
# Instance 1
AGENT_ID=replicante-001 DATABASE_PATH=agent1.db ./replicante

# Instance 2  
AGENT_ID=replicante-002 DATABASE_PATH=agent2.db ./replicante
```

### Load Balancing

Use a reverse proxy for API endpoints if the agent creates them:

```nginx
upstream replicante {
    server 127.0.0.1:8001;
    server 127.0.0.1:8002;
}
```

## Troubleshooting

### Agent Not Starting

```bash
# Check permissions
ls -l /usr/local/bin/replicante

# Test configuration
./replicante --test-config

# Verbose logging
RUST_LOG=debug ./replicante
```

### MCP Connection Issues

```bash
# Test MCP server directly
echo '{"method": "list_tools"}' | mcp-server-nostr

# Check if MCP servers are in PATH
which mcp-server-nostr
```

### Database Issues

```bash
# Check database integrity
sqlite3 replicante.db "PRAGMA integrity_check;"

# Backup database
cp replicante.db replicante.db.backup

# Reset database (warning: loses all memory)
rm replicante.db
```

## Best Practices

1. **Start Small**: Begin with minimal MCP servers and let the agent discover
2. **Monitor Costs**: Watch LLM API usage and set spending limits
3. **Regular Backups**: Backup the database to preserve agent memory
4. **Gradual Autonomy**: Start with limited tools, add more as you observe behavior
5. **Document Observations**: Keep notes on emergent behaviors and decisions

## Emergency Shutdown

If the agent needs to be stopped immediately:

```bash
# Stop systemd service
sudo systemctl stop replicante

# Kill process
pkill replicante

# Disable auto-restart
sudo systemctl disable replicante
```