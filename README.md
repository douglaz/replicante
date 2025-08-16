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

### Local Testing with Ollama

For local testing with Ollama (no API keys required):

```bash
# Install and start Ollama
ollama serve
ollama pull llama3.2:3b

# Run with example configuration
cp config-ollama-example.toml config.toml
cargo run --release
```

See [OLLAMA_EXAMPLE.md](OLLAMA_EXAMPLE.md) for detailed instructions on testing the enhanced MCP features and learning system locally.

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

## Building Static Binary

### Using Nix (Recommended)
```bash
# Build static musl binary
nix build .#replicante-static

# Verify it's static
ldd result/bin/replicante  # Should say "not a dynamic executable"
```

### Using Cargo directly
```bash
# Install musl target
rustup target add x86_64-unknown-linux-musl

# Build static binary
./scripts/build-static.sh

# Or manually:
RUSQLITE_BUNDLED=1 cargo build --release --target x86_64-unknown-linux-musl
```

## Deployment

```bash
# Build static binary
nix build .#replicante-static

# Deploy to server (works on any Linux x86_64)
scp result/bin/replicante server:/usr/local/bin/

# Run on server (no dependencies needed!)
ssh server 'ANTHROPIC_API_KEY=sk-... /usr/local/bin/replicante'
```

The static binary:
- Has no runtime dependencies
- Works on any Linux x86_64 system
- Includes bundled SQLite
- Uses rustls (no OpenSSL needed)
- Is optimized for size (~10-20MB)

## Development

### Setup

Git hooks are **automatically configured** when entering the nix development environment:

```bash
# Enter dev environment (auto-configures Git hooks)
nix develop
```

For non-nix users, manually configure hooks:
```bash
git config core.hooksPath .githooks
```

### Code Quality

#### Automated Checks
- **pre-commit**: Runs `cargo fmt --check` to ensure code is formatted
- **pre-push**: Runs both `cargo fmt --check` and `cargo clippy -- -D warnings`

#### Manual Checks
```bash
# Format code
cargo fmt

# Run linter
cargo clippy

# Run tests
cargo test
```

#### Managing Hooks
```bash
# Bypass hooks temporarily (not recommended)
git commit --no-verify
git push --no-verify

# Disable hooks
git config --unset core.hooksPath
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