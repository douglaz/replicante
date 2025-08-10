# Replicante Supervisor & Sandbox System

## Overview

The Replicante project now includes a comprehensive supervisor and sandbox system for safely testing and monitoring autonomous AI agents with increasing capabilities. This system provides multiple layers of security and observability.

## Architecture

### Components

1. **Supervisor Daemon** - Monitors and controls agent instances
2. **Sandbox Environment** - Isolated execution with resource limits
3. **Monitor System** - Real-time metrics and alerting
4. **Web Dashboard** - Visual monitoring interface

## Quick Start

### Running the Supervisor

```bash
# Start supervisor with default config
replicante supervisor start

# Start with custom config
replicante supervisor start --config config/supervisor.toml --web-port 8080

# Check status
replicante supervisor status
```

### Running a Sandboxed Agent

```bash
# Run with default sandbox
replicante sandbox

# Run with custom configs
replicante sandbox --config agent.toml --sandbox-config sandbox-strict.toml

# Connect to supervisor
replicante sandbox --supervisor http://localhost:8080
```

### Using Docker Compose

```bash
# Start entire stack
docker-compose up -d

# View logs
docker-compose logs -f

# Stop everything
docker-compose down
```

## Sandbox Modes

### Strict Mode
- Maximum isolation
- Minimal capabilities
- No network access or heavily filtered
- Restricted filesystem access
- Low resource limits

### Moderate Mode (Default)
- Balanced security
- Filtered network access
- Limited filesystem access
- Reasonable resource limits

### Permissive Mode
- Minimal restrictions
- Full monitoring
- Higher resource limits
- Used for trusted agents

## Configuration

### Sandbox Configuration (`sandbox.toml`)

```toml
[sandbox]
enabled = true
mode = "Moderate"

[sandbox.filesystem]
root = "/sandbox"
write_paths = ["/sandbox/workspace"]
max_file_size_mb = 10

[sandbox.network]
mode = "Filtered"
allowed_domains = ["api.anthropic.com"]
rate_limit_per_minute = 100

[sandbox.resources]
max_memory_mb = 512
max_cpu_percent = 50.0

[sandbox.mcp]
blocked_tools = ["shell:execute"]
```

### Supervisor Configuration (`supervisor.toml`)

```toml
[supervisor]
max_agents = 10
web_port = 8080

[supervisor.alerts]
max_cpu_percent = 80.0
max_memory_mb = 512
```

## Monitoring

### Web Dashboard
Access at `http://localhost:8080` when supervisor is running.

Features:
- Real-time agent status
- Resource usage graphs
- Alert notifications
- Event timeline
- Emergency controls

### CLI Monitoring

```bash
# View metrics
replicante monitor metrics --agent-id abc123

# Show recent events
replicante monitor events -n 100

# Show alerts
replicante monitor alerts

# Export audit log
replicante monitor audit --export audit.json
```

## Security Features

### Process Isolation
- Linux namespaces (when available)
- Capability dropping
- Non-root execution
- Read-only root filesystem

### Network Filtering
- Domain whitelisting/blacklisting
- Port restrictions
- Rate limiting
- Connection limits

### Filesystem Restrictions
- Chroot-like isolation
- Path sanitization
- Size limits
- Write restrictions

### Resource Limits
- Memory limits
- CPU throttling
- Process limits
- File descriptor limits

### MCP Tool Filtering
- Tool whitelisting/blacklisting
- Rate limiting per tool
- Pattern matching
- Server restrictions

## Emergency Procedures

### Stop Agent
```bash
replicante supervisor stop agent-123
```

### Emergency Kill
```bash
replicante supervisor kill agent-123
```

### Quarantine Agent
```bash
replicante supervisor quarantine agent-123
```

### Kill All Agents
Access dashboard and use "Emergency Stop All" button.

## Testing Scenarios

### Progressive Testing
1. Start with strict sandbox
2. Gradually increase capabilities
3. Monitor for violations
4. Adjust policies based on behavior

### Example Test Progression

```bash
# Level 1: Minimal capabilities
replicante sandbox --sandbox-config sandbox-strict.toml

# Level 2: Add filesystem access
replicante sandbox --sandbox-config sandbox-filesystem.toml

# Level 3: Add network access
replicante sandbox --sandbox-config sandbox-network.toml

# Level 4: Full moderate sandbox
replicante sandbox --sandbox-config sandbox.toml
```

## Alerts and Violations

### Alert Types
- `HighResourceUsage` - CPU/Memory exceeded
- `SuspiciousToolUsage` - Unusual tool patterns
- `UnauthorizedAccess` - Blocked filesystem access
- `NetworkAnomaly` - Suspicious network activity
- `PrivilegeEscalation` - Attempted privilege increase

### Viewing Violations
```bash
# In supervisor logs
replicante supervisor logs agent-123

# Via monitoring
replicante monitor alerts
```

## Docker Deployment

### Building Images
```bash
# Build agent image
docker build -t replicante:latest .

# Build supervisor image
docker build -f Dockerfile.supervisor -t replicante-supervisor:latest .
```

### Running Stack
```bash
# Start with docker-compose
docker-compose up -d

# Scale agents
docker-compose up -d --scale agent-sandboxed=3

# View dashboard
open http://localhost:8080
```

## Development

### Adding New Sandbox Restrictions

1. Edit `src/sandbox/mod.rs` to add new restriction types
2. Implement check in appropriate module (filesystem.rs, network.rs, etc.)
3. Add configuration options
4. Update tests

### Adding Monitor Metrics

1. Edit `src/supervisor/monitor.rs`
2. Add new metric types
3. Update dashboard to display
4. Add export format

## Troubleshooting

### Agent Won't Start
- Check sandbox permissions
- Verify configuration files
- Check supervisor logs

### High Resource Usage
- Review resource limits
- Check for resource leaks
- Enable CPU throttling

### Network Blocked
- Check allowed domains
- Verify port restrictions
- Review rate limits

### Dashboard Not Loading
- Verify supervisor is running
- Check port 8080 is not in use
- Review firewall settings

## Best Practices

1. **Always start with strict sandbox** for new capabilities
2. **Monitor continuously** during testing
3. **Document all violations** for analysis
4. **Use version control** for configurations
5. **Test emergency procedures** regularly
6. **Keep audit logs** for compliance
7. **Review alerts daily** for patterns
8. **Update security policies** based on findings

## Future Enhancements

- [ ] Kubernetes deployment support
- [ ] Multi-host agent distribution
- [ ] Advanced threat detection
- [ ] Automated policy learning
- [ ] Integration with SIEM systems
- [ ] Agent behavior profiling
- [ ] Automated incident response
- [ ] Blockchain audit logging

## Support

For issues or questions:
1. Check the logs: `docker-compose logs`
2. Review configuration files
3. Consult this documentation
4. Open an issue on GitHub