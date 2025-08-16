# Running Replicante with Ollama - Testing Enhanced Features

This guide demonstrates how to run Replicante locally with Ollama to test the new MCP integration enhancements and learning algorithms.

## Prerequisites

### 1. Install Ollama
```bash
# Install Ollama if not already installed
curl -fsSL https://ollama.ai/install.sh | sh

# Start Ollama service
ollama serve  # Run in a separate terminal
```

### 2. Pull a Model
```bash
# Recommended: Llama 3.2 3B (fast, 2GB download)
ollama pull llama3.2:3b

# Alternative models:
ollama pull llama2:7b     # Better reasoning, slower
ollama pull mistral:7b    # Good balance
ollama pull phi3:mini     # Very fast, smaller
```

## Quick Start with Enhanced Features

### 1. Use the Example Configuration

We provide a pre-configured example that showcases the new features:

```bash
# Copy the example configuration
cp config-ollama-example.toml config.toml

# Run the agent
cargo run --release
```

### 2. What You'll See

The enhanced features in action:

#### Retry Logic
```
INFO replicante::mcp: Starting MCP server process: filesystem-mock
ERROR replicante::mcp: Failed to start MCP server filesystem (attempt 1/3): Failed to spawn
INFO replicante::mcp: Starting MCP server process: filesystem-mock  
ERROR replicante::mcp: Failed to start MCP server filesystem (attempt 2/3): Failed to spawn
INFO replicante::mcp: Starting MCP server process: filesystem-mock
ERROR replicante::mcp: Failed to start MCP server filesystem (attempt 3/3): Failed to spawn
WARN replicante::mcp: Giving up on MCP server filesystem after 3 attempts
```

#### Health Monitoring
The agent will periodically check server health (every 60 seconds by default) and attempt to restart unhealthy servers:
```
WARN replicante::mcp: Health check failed for filesystem: timeout
WARN replicante::mcp: Server filesystem appears unhealthy after 3 consecutive failures
INFO replicante::mcp: Attempting to restart MCP server
```

#### Learning System
Watch the agent learn from its actions:
```
INFO replicante: Learning from 5 recent decisions
INFO replicante: Found learned pattern with 0.85 confidence: explore
INFO replicante: Using learned action based on past success
```

## Monitoring the Learning System

### View Learning Metrics
```bash
# Check what patterns the agent has learned
sqlite3 replicante.db "SELECT * FROM action_patterns ORDER BY confidence DESC;"

# View learning metrics
sqlite3 replicante.db "SELECT * FROM learning_metrics;"

# See decision history with patterns
sqlite3 replicante.db "SELECT thought, action FROM decisions WHERE thought LIKE '%[learned]%';"
```

### Example Output
After running for a few minutes, you'll see:
- **Action patterns**: Context-based patterns with confidence scores
- **Learning metrics**: Success rates for decisions and tools
- **Adapted behavior**: The agent using successful patterns more frequently

## Custom Configuration

Create your own `config.toml` to test specific scenarios:

```toml
database_path = "replicante-ollama.db"

[agent]
id = "replicante-ollama-test"
log_level = "info"
reasoning_interval_secs = 10
initial_goals = """
Your primary goals are:
1. Test the learning system by exploring available tools
2. Execute tools and track their success/failure patterns
3. Use learned patterns to improve decision-making
4. Monitor your own health metrics
5. Demonstrate retry and health monitoring features
"""

[llm]
provider = "ollama"
model = "llama3.2:3b"
api_url = "http://localhost:11434"
temperature = 0.7
max_tokens = 2000

# MCP servers with enhanced configuration
[[mcp_servers]]
name = "test_server"
transport = "stdio"
command = "echo"
args = ["test"]
retry_attempts = 3              # Will retry 3 times
retry_delay_ms = 1000           # Wait 1 second between retries
health_check_interval_secs = 30 # Check health every 30 seconds
```

## Testing Specific Features

### 1. Test Retry Logic
Add a server that will fail initially:
```toml
[[mcp_servers]]
name = "flaky_server"
transport = "stdio"
command = "sh"
args = ["-c", "exit 1"]  # Always fails
retry_attempts = 5
retry_delay_ms = 2000
```

### 2. Test Learning System
Set goals that encourage pattern learning:
```toml
initial_goals = """
1. Try the same action multiple times
2. Track which actions succeed most often
3. Prefer actions with higher success rates
4. Build a knowledge base of effective strategies
"""
```

### 3. Test Health Monitoring
Run the agent and kill an MCP server process:
```bash
# In another terminal, find and kill an MCP process
ps aux | grep mcp
kill -9 <pid>
# Watch the agent detect and attempt to restart it
```

## Docker Testing

To test in Docker with Ollama on the host:

```bash
# Build the Docker image
docker build -t replicante-test .

# Run with host networking (easiest for Ollama access)
docker run -it --network host \
  -v $(pwd)/config-ollama-example.toml:/config/config.toml \
  -v /tmp/replicante-data:/data \
  -e RUST_LOG=info \
  replicante-test

# Or use bridge networking with explicit Ollama URL
docker run -it \
  -v $(pwd)/config-ollama-example.toml:/config/config.toml \
  -v /tmp/replicante-data:/data \
  -e RUST_LOG=info \
  -e OLLAMA_HOST=http://172.17.0.1:11434 \
  replicante-test
```

## Performance Monitoring

### Check Agent Performance
```bash
# View decision success rate over time
sqlite3 replicante.db "
  SELECT 
    datetime(created_at) as time,
    COUNT(*) as decisions,
    SUM(CASE WHEN result IS NOT NULL THEN 1 ELSE 0 END) as completed
  FROM decisions 
  GROUP BY date(created_at)
  ORDER BY time DESC;
"

# View tool success rates
sqlite3 replicante.db "
  SELECT 
    tool_name, 
    success_rate,
    datetime(last_used) as last_used
  FROM capabilities 
  ORDER BY success_rate DESC;
"
```

### Watch Real-time Learning
```bash
# Monitor learning in real-time
watch -n 5 'sqlite3 replicante.db "
  SELECT metric_name, metric_value 
  FROM learning_metrics 
  ORDER BY updated_at DESC 
  LIMIT 10;
"'
```

## Troubleshooting

### Ollama Connection Issues
```bash
# Verify Ollama is running
curl http://localhost:11434/api/tags

# Check available models
ollama list

# Test model directly
ollama run llama3.2:3b "Hello, are you working?"
```

### Learning System Not Working
```bash
# Check if tables were created
sqlite3 replicante.db ".tables"
# Should show: action_patterns capabilities decisions learning_metrics memory

# Verify data is being recorded
sqlite3 replicante.db "SELECT COUNT(*) FROM action_patterns;"
```

### High Memory Usage
```bash
# Use a smaller model
ollama pull llama3.2:1b  # Tiny model
ollama pull gemma:2b      # Google's small model

# Or limit Ollama memory
OLLAMA_MAX_MEMORY=4GB ollama serve
```

## Expected Results

After running for 5-10 minutes with the enhanced features:

1. **Retry Statistics**: You'll see multiple retry attempts in logs with configured delays
2. **Health Checks**: Periodic health check logs (based on configured intervals)
3. **Learning Data**:
   - 10-20 action patterns recorded
   - 5-10 learning metrics tracked
   - Confidence scores improving for successful patterns
4. **Adaptive Behavior**: The agent will start preferring actions that have worked before

## Next Steps

1. **Experiment with Goals**: Modify `initial_goals` to test different learning scenarios
2. **Add Real MCP Servers**: Replace mock servers with actual implementations
3. **Tune Parameters**: Adjust retry attempts, delays, and health check intervals
4. **Extended Running**: Let it run for hours to see long-term learning patterns
5. **Multiple Agents**: Run multiple instances to see if they learn differently

## Summary

This example demonstrates:
- ✅ Configurable retry logic with attempts and delays
- ✅ Health monitoring with automatic recovery
- ✅ Pattern recognition and learning system
- ✅ Confidence-based decision making
- ✅ Performance metrics tracking
- ✅ Local LLM integration via Ollama

The agent is now capable of learning from experience and adapting its behavior based on what works!