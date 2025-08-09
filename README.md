# Replicante: Autonomous AI Agent

An experimental autonomous AI agent that discovers its own capabilities and decides its own survival strategy.

## Philosophy

Replicante is a minimal autonomous agent that:
- **Thinks** via LLM integration (Anthropic, OpenAI, Ollama)
- **Acts** via MCP (Model Context Protocol) tools
- **Remembers** via SQLite persistence
- **Decides** everything else autonomously

We don't tell Replicante what services to provide or how to survive - it figures that out itself.

## Quick Start

### Using Nix (Recommended)

```bash
# Enter development environment
nix develop

# Build and run
cargo run

# Build static binary for deployment
nix build .#replicante-static
```

### Manual Setup

```bash
# Set LLM credentials
export ANTHROPIC_API_KEY=sk-...
# or
export OPENAI_API_KEY=sk-...

# Run the agent
cargo run --release
```

## Configuration

Edit `config.toml` to configure:
- **Initial goals** - Define what the agent should focus on
- LLM provider (Anthropic, OpenAI, Ollama)
- MCP servers (tools available to the agent)
- Database path

### Custom Goals (New!)

Define what your agent should focus on:

```toml
[agent]
initial_goals = """
Your primary goals are:
1. Learn about your environment
2. Provide value to others
3. Build knowledge over time
4. Generate sustainable revenue
5. Survive and grow
"""
```

See `CUSTOM_GOALS.md` for detailed examples and `config-examples.toml` for pre-made configurations.

## Architecture

```
src/
├── main.rs    # Reasoning loop
├── llm.rs     # LLM abstraction
├── mcp.rs     # Tool usage via MCP
├── state.rs   # Persistence
└── config.rs  # Configuration
```

## What It Does

1. **Observes** - Gathers information about its environment
2. **Thinks** - Uses LLM to reason about what to do
3. **Decides** - Chooses actions based on reasoning
4. **Acts** - Executes actions via MCP tools
5. **Learns** - Remembers outcomes for future decisions

## Deployment

```bash
# Build static binary
nix build .#replicante-static

# Deploy to server
scp result/bin/replicante server:/usr/local/bin/

# Run on server
ssh server 'ANTHROPIC_API_KEY=sk-... /usr/local/bin/replicante'
```

## MCP Tools

The agent can discover and use tools via MCP servers:
- **Nostr** - Social network interaction, DVMs
- **Filesystem** - Local file operations
- **HTTP** - Web requests
- **Bitcoin/Lightning** - Payments

## Emergent Behavior

We don't define what services Replicante provides. Instead, it:
- Discovers opportunities through observation
- Decides what services to offer
- Figures out how to implement them
- Sets its own prices
- Finds its own customers

## License

MIT