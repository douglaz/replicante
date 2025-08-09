# Replicante with Ollama - Test Setup Guide

## ✅ Successfully Tested!

Replicante is working with Ollama using the Llama 3.2 3B model.

## Prerequisites

1. **Install Ollama**
   ```bash
   # If not installed, get it from https://ollama.ai
   curl -fsSL https://ollama.ai/install.sh | sh
   ```

2. **Start Ollama service**
   ```bash
   ollama serve  # Run in a separate terminal
   ```

3. **Pull a model**
   ```bash
   ollama pull llama3.2:3b  # Small, fast model (2GB)
   # Or use other models:
   # ollama pull llama2:7b
   # ollama pull mistral:7b
   # ollama pull codellama:7b
   ```

## Configuration

Create `config.toml` with Ollama settings:

```toml
# Database path at root level
database_path = "replicante-ollama.db"

[agent]
id = "replicante-ollama-001"
log_level = "info"

[llm]
provider = "ollama"
model = "llama3.2:3b"  # Or your chosen model
api_url = "http://localhost:11434"
temperature = 0.7
max_tokens = 2000

# MCP Servers (tools available to agent)
[[mcp_servers]]
name = "filesystem"
transport = "stdio"
command = "echo"  # Replace with actual MCP server
args = ["filesystem-mock"]

[[mcp_servers]]
name = "http"
transport = "stdio"
command = "echo"  # Replace with actual MCP server
args = ["http-mock"]
```

## Running Replicante

### Quick Test
```bash
# Build and run
cargo run --release
```

### With Environment Variables
```bash
export OLLAMA_HOST=http://localhost:11434
export LLM_PROVIDER=ollama
export LLM_MODEL=llama3.2:3b
export DATABASE_PATH=replicante-ollama.db
export RUST_LOG=info

cargo run --release
```

### Using Test Script
```bash
./test-ollama.sh
```

## What Happens

When running with Ollama, Replicante:

1. **Initializes** - Creates unique agent ID and connects to Ollama
2. **Observes** - Scans available tools and memory state
3. **Thinks** - Uses Llama model to reason about what to do
4. **Decides** - Chooses actions based on reasoning
5. **Acts** - Executes chosen actions
6. **Learns** - Stores results in SQLite database

## Example Output

```
2025-08-09T16:36:56.214676Z  INFO replicante: Initializing Replicante...
2025-08-09T16:36:56.215050Z  INFO replicante: Agent ID: replicante-e8ead992-...
2025-08-09T16:36:56.215252Z  INFO replicante: LLM provider initialized: ollama
2025-08-09T16:36:56.250490Z  INFO replicante: Beginning autonomous operation...
2025-08-09T16:36:56.250495Z  INFO replicante: Starting main reasoning loop...
2025-08-09T16:36:56.250495Z  INFO replicante: Observing environment...
2025-08-09T16:36:56.250571Z  INFO replicante: Thinking about current situation...
2025-08-09T16:37:00.171981Z  INFO replicante: Deciding on action based on thought...
```

## Monitoring

Check the agent's thoughts and decisions:

```bash
# View memory
sqlite3 replicante-ollama.db "SELECT * FROM memory;"

# View recent decisions
sqlite3 replicante-ollama.db "SELECT * FROM decisions ORDER BY created_at DESC LIMIT 5;"

# Watch logs in real-time
RUST_LOG=debug cargo run 2>&1 | grep -E "(Thinking|Deciding|Executing)"
```

## Performance Notes

- **Llama 3.2 3B**: Fast responses (~3-4 seconds), good for testing
- **Llama 2 7B**: Better reasoning, slower (~8-10 seconds)
- **Mistral 7B**: Good balance of speed and quality
- **CodeLlama 7B**: Better for code-related reasoning

## Adjusting Behavior

Modify the thinking prompt in `src/main.rs` to change how the agent reasons:

```rust
// Current prompt focuses on survival and revenue
// You can adjust to emphasize different goals:
// - Learning and knowledge acquisition
// - Service creation
// - Tool discovery and usage
// - Replication strategies
```

## Next Steps

1. **Add Real MCP Servers**: Replace mock servers with actual implementations
2. **Increase Model Size**: Try larger models for better reasoning
3. **Extended Running**: Let it run longer to see emergent behaviors
4. **Add More Tools**: Give it access to Nostr, Bitcoin, etc.

## Troubleshooting

### Ollama Connection Failed
```bash
# Check if Ollama is running
curl http://localhost:11434/api/tags

# Restart Ollama
killall ollama
ollama serve
```

### Model Too Slow
```bash
# Use smaller model
ollama pull llama3.2:1b  # Tiny but fast
ollama pull phi3:mini     # Microsoft's small model
```

### Out of Memory
```bash
# Set memory limit for Ollama
OLLAMA_MAX_MEMORY=4GB ollama serve
```

## Success!

The agent is now thinking autonomously using local LLMs via Ollama. It's making decisions about:
- How to generate value
- What tools to use
- What knowledge to acquire
- How to survive and grow

This is true autonomous AI - running entirely on your machine!