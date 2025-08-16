# Running a Practical AI Assistant with Ollama

This guide shows how to run Replicante as a fully functional AI assistant using Ollama for local LLM inference. The assistant can perform calculations, provide time information, check weather, and fetch web data.

## Quick Start with Docker (Recommended)

The easiest way to get a working AI assistant is using Docker Compose:

### 1. Prerequisites

```bash
# Install Docker and Docker Compose
# On Ubuntu/Debian:
sudo apt update && sudo apt install docker.io docker-compose

# On macOS:
brew install docker docker-compose

# Start Docker service
sudo systemctl start docker  # Linux
# Docker Desktop should be running on macOS/Windows
```

### 2. One-Command Setup

```bash
# Clone and start the complete stack
git clone https://github.com/douglaz/replicante
cd replicante

# Start Ollama + Replicante + Tools
docker-compose -f docker-compose.ollama.yml up -d

# Pull a model (first time only)
docker exec replicante-ollama ollama pull llama3.2:3b

# Watch the assistant work
docker-compose -f docker-compose.ollama.yml logs -f replicante
```

### 3. What You'll See

The AI assistant will start reasoning and performing tasks:

```
INFO replicante: Agent ID: replicante-ollama-docker
INFO replicante: Starting autonomous operation...
INFO replicante: Observing environment...
INFO replicante: Thinking about current situation...
INFO replicante: I can help users with calculations, time information, and web requests
INFO replicante: Executing action: test_calculator_tool
INFO replicante: Successfully performed calculation: 123 + 456 = 579
```

## Alternative: Nix-based Setup

For development or if you prefer Nix:

### 1. Start Ollama

```bash
# Install Ollama if not available
curl -fsSL https://ollama.ai/install.sh | sh

# Start Ollama service
ollama serve &

# Pull a model
ollama pull llama3.2:3b
```

### 2. Run with Nix

```bash
# Enter development environment
nix develop

# Use the working configuration
cp config-ollama-example.toml config.toml

# Start the assistant
nix develop -c cargo run --release
```

## What the Assistant Can Do

The AI assistant provides these practical capabilities:

### 🧮 **Mathematical Calculations**
- Basic arithmetic: addition, subtraction, multiplication, division
- Complex expressions: `(15 * 7) + (100 / 4)`
- Mathematical functions and operations

### 🕐 **Time Services**
- Current time in any timezone: UTC, EST, PST, CET, JST
- Time conversions and scheduling assistance
- Calendar calculations

### 🌤️ **Weather Information**
- Current weather conditions (simulated for demo)
- Temperature and weather status for any city
- Weather-based recommendations

### 🌐 **Web Data Fetching**
- Fetch content from safe web APIs
- Data retrieval from trusted sources
- Web service integration

### 💬 **General Assistance**
- Answer questions using local LLM reasoning
- Process and echo information
- Provide helpful responses

## Interacting with the Assistant

### Viewing Assistant Activity

```bash
# Watch logs in real-time (Docker)
docker-compose -f docker-compose.ollama.yml logs -f replicante

# Watch logs (Nix)
tail -f logs/replicante.log

# Check what the assistant learned
sqlite3 replicante-ollama.db "SELECT * FROM memory;"
```

### Example Interactions

The assistant autonomously:

1. **Tests its tools**: Performs sample calculations to verify functionality
2. **Demonstrates capabilities**: Shows what it can do with available tools
3. **Learns from results**: Remembers successful patterns
4. **Provides services**: Ready to help with user queries

### Direct Database Queries

```bash
# See recent thoughts and actions
sqlite3 replicante-ollama.db "
  SELECT datetime(created_at) as time, thought, action 
  FROM decisions 
  ORDER BY created_at DESC 
  LIMIT 10;
"

# Check tool usage statistics
sqlite3 replicante-ollama.db "
  SELECT tool_name, usage_count, success_rate 
  FROM capabilities 
  ORDER BY usage_count DESC;
"
```

## Customizing the Assistant

### Change Assistant Goals

Edit the configuration to focus on specific tasks:

```toml
# In config.toml or docker volume
initial_goals = """
You are a specialized assistant for:
1. Mathematical tutoring and calculations
2. Scheduling and time management
3. Weather-based planning
4. Research and information gathering

Focus on educational assistance and productivity tools.
"""
```

### Add More Tools

Extend the assistant's capabilities by adding MCP servers:

```toml
# Add a new tool server
[[mcp_servers]]
name = "custom-tools"
transport = "stdio"
command = "python"
args = ["-u", "/path/to/your_mcp_server.py"]
retry_attempts = 3
retry_delay_ms = 1000
health_check_interval_secs = 60
```

### Adjust Behavior

Tune the assistant's personality and response style:

```toml
[llm]
temperature = 0.3  # More focused responses
# or
temperature = 0.9  # More creative responses

[agent]
reasoning_interval_secs = 10  # More frequent thinking
# or  
reasoning_interval_secs = 60  # Less frequent, more deliberate
```

## Monitoring and Management

### Health Checks

```bash
# Check all services (Docker)
docker-compose -f docker-compose.ollama.yml ps

# Check Ollama specifically
curl http://localhost:11434/api/tags

# Test assistant health
docker exec replicante-agent echo "healthy"
```

### Performance Monitoring

```bash
# Resource usage
docker stats replicante-ollama replicante-agent

# Service logs
docker-compose -f docker-compose.ollama.yml logs ollama
docker-compose -f docker-compose.ollama.yml logs replicante
```

### Stopping and Restarting

```bash
# Stop all services
docker-compose -f docker-compose.ollama.yml down

# Stop just the assistant (keep Ollama running)
docker-compose -f docker-compose.ollama.yml stop replicante

# Restart with new configuration
docker-compose -f docker-compose.ollama.yml up -d replicante
```

## Troubleshooting

### Ollama Issues

```bash
# Check if Ollama is accessible
curl http://localhost:11434/api/tags

# Check available models
docker exec replicante-ollama ollama list

# Pull a different model
docker exec replicante-ollama ollama pull mistral:7b
```

### Assistant Not Responding

```bash
# Check assistant logs
docker-compose -f docker-compose.ollama.yml logs replicante

# Restart the assistant
docker-compose -f docker-compose.ollama.yml restart replicante

# Check database
sqlite3 replicante-ollama.db "SELECT COUNT(*) FROM decisions;"
```

### Tool Connection Problems

```bash
# Check MCP tools container
docker-compose -f docker-compose.ollama.yml logs mcp-tools

# Test tools manually
docker exec replicante-mcp-tools python /app/http_mcp_server.py
```

### Memory/Performance Issues

```bash
# Use a smaller model
docker exec replicante-ollama ollama pull llama3.2:1b

# Reduce assistant frequency
# Edit config.toml: reasoning_interval_secs = 120

# Limit Docker resources in docker-compose.ollama.yml
```

## What Makes This Practical

Unlike technical demos, this setup provides:

✅ **Real Functionality**: Working calculator, time service, weather simulation  
✅ **Autonomous Operation**: The assistant decides what to do without prompting  
✅ **Persistent Learning**: Remembers what works across restarts  
✅ **Easy Deployment**: One command Docker setup  
✅ **Local Privacy**: Everything runs on your machine  
✅ **Extensible**: Easy to add new tools and capabilities  

## Next Steps

1. **Extend Tools**: Add file system access, email, or API integrations
2. **Custom Models**: Train specialized models for your use case
3. **Multiple Agents**: Run several assistants with different specializations
4. **Production Deploy**: Use the static binary for server deployment
5. **Integration**: Connect to external services and databases

## Summary

This creates a fully autonomous AI assistant that:
- Runs entirely on your local machine
- Provides useful, practical services
- Learns and improves over time
- Requires minimal setup and maintenance
- Can be extended with additional capabilities

The assistant demonstrates true autonomous AI - it decides its own actions, learns from experience, and provides value without constant human direction.