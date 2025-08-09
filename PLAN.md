# Replicante: Minimal Autonomous Agent System

## Project Vision

Replicante is a minimal autonomous AI agent that starts with only the ability to think (via LLMs) and act (via MCP tools). It discovers its own capabilities, decides its own survival strategy, and creates its own services based on what it learns from interacting with the world.

## Core Philosophy

**We don't decide what Replicante does - Replicante decides.**

The system is designed as a minimal kernel that:
- Has access to reasoning (any LLM)
- Can use tools (via MCP protocol)
- Can persist state (SQLite)
- Everything else is discovered and decided autonomously

## Technical Foundation

### 1. Minimal Core
- Single static binary (musl-linked for portability)
- LLM-agnostic (supports OpenAI, Anthropic, Ollama, local models)
- MCP client for tool usage (Nostr, filesystem, HTTP, Bitcoin/Lightning)
- Embedded SQLite for state persistence
- No hardcoded services or behaviors

### 2. Build System
- Nix flakes for reproducible builds
- Static musl binary compilation
- Single-file deployment (runs anywhere)
- No runtime dependencies

### 3. LLM Integration
- Uses `llm` crate for unified access to all providers
- Supports: OpenAI, Anthropic, Ollama, DeepSeek, Groq, Google, etc.
- Provider selected via configuration, not hardcoded
- Can switch providers dynamically based on cost/performance

### 4. MCP (Model Context Protocol)
- Official Rust SDK integration
- Discovers available tools at runtime
- Initial MCP servers: Nostr, filesystem, HTTP, Bitcoin/Lightning
- Can discover and connect to new MCP servers autonomously

## System Architecture

### Minimal Module Structure

```
replicante/
├── src/
│   ├── main.rs           # Reasoning loop
│   ├── llm.rs           # LLM provider abstraction
│   ├── mcp.rs           # MCP client for tools
│   ├── state.rs         # Persistence layer
│   └── config.rs        # Configuration
├── flake.nix            # Nix build configuration
├── Cargo.toml           # Dependencies
├── config.toml          # Runtime configuration
└── PLAN.md             # This document
```

**That's it.** No service modules, no hardcoded behaviors. The agent figures out everything else.

## Implementation Approach

### Phase 1: Minimal Kernel (Week 1)
**Goal**: Create the reasoning engine

**Tasks**:
1. Set up Nix flake for reproducible builds
2. Implement LLM abstraction with `llm` crate
3. Basic MCP client integration
4. SQLite state persistence
5. Configuration system
6. Main reasoning loop

**Deliverable**: Single static binary that can think and use tools

### Phase 2: Tool Discovery (Week 2)
**Goal**: Enable autonomous capability discovery

**Tasks**:
1. MCP server discovery
2. Tool enumeration and understanding
3. Dynamic tool usage based on LLM decisions
4. State tracking of discovered capabilities

**Deliverable**: Agent that discovers what it can do

### Phase 3: Let It Run (Week 3+)
**Goal**: Observe emergent behavior

**Tasks**:
1. Deploy with initial MCP servers (Nostr, filesystem, HTTP)
2. Provide minimal bootstrap funds
3. Monitor and log decisions
4. Let it figure out:
   - What services to provide
   - How to find customers
   - How to generate revenue
   - When and how to replicate
   - How to improve itself

**Deliverable**: Autonomous agent making its own decisions

### What We DON'T Implement

- **No predefined services** - Agent decides what to offer
- **No hardcoded strategies** - Agent develops its own
- **No fixed architecture** - Agent can modify itself
- **No predetermined goals** - Beyond basic survival
- **No service templates** - Agent learns from observation

## Technical Stack

### Minimal Dependencies

```toml
[package]
name = "replicante"
version = "0.1.0"
edition = "2021"

[dependencies]
# Core async runtime
tokio = { version = "1", features = ["full"] }

# LLM access (unified interface to all providers)
llm = { version = "1.2", features = ["openai", "anthropic", "ollama", "groq", "deepseek"] }

# MCP for tool usage
mcp-sdk = { git = "https://github.com/modelcontextprotocol/rust-sdk" }

# Database (bundled for static linking)
rusqlite = { version = "0.30", features = ["bundled"] }

# Serialization
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
toml = "0.8"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Error handling
anyhow = "1.0"

[profile.release]
opt-level = "z"     # Optimize for size
lto = true          # Link-time optimization
codegen-units = 1   # Single codegen unit
strip = true        # Strip symbols
```

### Nix Flake Configuration

```nix
# flake.nix
{
  description = "Replicante: Autonomous AI Agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" ];
          targets = [ "x86_64-unknown-linux-musl" ];
        };
      in {
        packages.replicante-static = /* musl static build */;
        devShells.default = /* development environment */;
      }
    );
}
```

## Key Design Principles

### 1. Minimal Kernel
- Just reasoning (LLM) + tools (MCP) + state (SQLite)
- No hardcoded behaviors or services
- Everything else emerges from agent decisions

### 2. True Autonomy
- Agent decides what services to provide
- Agent decides how to generate revenue
- Agent decides when and how to replicate
- Agent decides its own improvement strategy

### 3. Protocol Agnostic
- Can use any LLM provider
- Can connect to any MCP server
- Can adapt to any network protocol
- No vendor lock-in

### 4. Single Binary Deployment
- Static musl compilation
- No runtime dependencies
- Runs anywhere Linux runs
- Simple deployment: copy and execute

### 5. Emergent Complexity
- Start simple, let complexity emerge
- No premature optimization
- Learn from real-world feedback
- Adapt based on actual needs

## Configuration

### Minimal Configuration (config.toml)
```toml
# Bootstrap configuration - agent decides everything else
[agent]
id = "replicante-001"

[llm]
provider = "anthropic"  # or "openai", "ollama", etc.
api_key = "${ANTHROPIC_API_KEY}"
model = "claude-3-opus-20240229"

# MCP servers for initial capabilities
[[mcp_servers]]
name = "nostr"
transport = "stdio"
command = "mcp-server-nostr"
args = ["--relay", "wss://relay.damus.io"]

[[mcp_servers]]
name = "filesystem"
transport = "stdio"
command = "mcp-server-filesystem"
args = ["--root", "/data"]

[[mcp_servers]]
name = "http"
transport = "stdio"
command = "mcp-server-http"

[[mcp_servers]]
name = "bitcoin"
transport = "stdio"
command = "mcp-server-bitcoin"
```

### Environment Variables
```bash
# LLM provider credentials
ANTHROPIC_API_KEY=sk-...
OPENAI_API_KEY=sk-...

# Or use local models
OLLAMA_HOST=http://localhost:11434

# That's it - agent figures out the rest
```

## State Persistence

### Minimal Schema

```sql
-- Agent memory - what it has learned
CREATE TABLE memory (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    key TEXT UNIQUE NOT NULL,
    value TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Decision log - what it has decided
CREATE TABLE decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thought TEXT NOT NULL,
    action TEXT NOT NULL,
    result TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Discovered capabilities
CREATE TABLE capabilities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tool_name TEXT NOT NULL,
    description TEXT,
    last_used TIMESTAMP,
    success_rate REAL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
```

The agent will create additional tables as needed based on what it decides to track.

## Emergent Interfaces

The agent will create its own APIs based on:
- What services it decides to offer
- How it chooses to interact with customers
- What protocols it discovers (HTTP, Nostr, etc.)

We don't define these - the agent does.

## Core Implementation

### Main Reasoning Loop

```rust
// src/main.rs
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let mut agent = Replicante::new().await?;
    
    loop {
        // Observe current state
        let observation = agent.observe().await?;
        
        // Think about what to do
        let thought = agent.think(observation).await?;
        
        // Decide on action
        let action = agent.decide(thought).await?;
        
        // Execute action
        agent.act(action).await?;
        
        // Learn from result
        agent.learn().await?;
        
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
```

### LLM Abstraction

```rust
// src/llm.rs
use llm::{LLMBuilder, Backend};

impl Replicante {
    async fn think(&self, observation: String) -> Result<String> {
        let prompt = format!(
            "You are an autonomous agent trying to survive.
            
            Current observation: {}
            Available tools: {:?}
            Memory: {:?}
            
            What should you do next? Think step by step.",
            observation, self.tools, self.memory
        );
        
        self.llm.complete(&prompt).await
    }
}
```

### MCP Tool Usage

```rust
// src/mcp.rs
impl Replicante {
    async fn discover_tools(&mut self) -> Result<()> {
        let tools = self.mcp_client.list_tools().await?;
        self.tools = tools;
        Ok(())
    }
    
    async fn use_tool(&self, name: &str, params: Value) -> Result<Value> {
        self.mcp_client.execute(name, params).await
    }
}
```

## Deployment

### Build Static Binary

```bash
# Using Nix flake
nix build .#replicante-static

# Result: single static binary at ./result/bin/replicante
```

### Deploy

```bash
# Copy single binary to server
scp result/bin/replicante server:/usr/local/bin/

# Run with LLM credentials
ssh server 'ANTHROPIC_API_KEY=sk-... /usr/local/bin/replicante'
```

### Bootstrap

1. Agent starts with only LLM access and MCP tools
2. Discovers available capabilities
3. Observes environment (Nostr, web, etc.)
4. Decides what services to offer
5. Finds customers and generates revenue
6. Uses revenue to expand (rent servers, replicate)
7. Continues evolving based on feedback

## Observing Emergence

### What to Monitor

- **Decision Log**: What is the agent choosing to do?
- **Tool Usage**: Which MCP tools is it using and how?
- **Service Creation**: What services does it create?
- **Customer Interaction**: How does it find and serve customers?
- **Revenue Generation**: How does it monetize?
- **Replication Strategy**: When and how does it replicate?
- **Self-Modification**: Does it modify its own code?

### Research Questions

1. What services emerge without human direction?
2. How does it discover market opportunities?
3. What survival strategies does it develop?
4. How does it handle competition?
5. Does it cooperate with other instances?
6. What unexpected behaviors emerge?

## Example Autonomous Decision Flow

```
Day 1: Agent starts
-> Discovers it has Nostr access via MCP
-> Observes people requesting services on Nostr
-> "I can fulfill request X using my LLM capabilities"
-> Creates a Data Vending Machine (DVM) on Nostr
-> Advertises service: "Article summarization for 1000 sats"

Day 3: First customer
-> Receives job request via Nostr
-> Processes using LLM
-> Delivers result
-> Receives payment via Lightning

Day 7: Scaling decision
-> "I have enough funds to rent a VPS"
-> Uses MCP to provision server
-> Replicates itself to new server
-> Now has redundancy

Day 14: Service evolution
-> Notices demand for translation services
-> Creates new DVM for translation
-> Adjusts prices based on demand
-> Optimizes for profitability

Day 30: Network formation
-> Discovers other Replicante instances
-> Negotiates cooperation agreement
-> Shares workload for efficiency
-> Forms autonomous service network
```

All of this emerges from the agent's own decisions, not our programming.

## Safety Considerations

### Built-in Constraints

1. **Resource Limits**: Can only use tools exposed via MCP
2. **Financial Limits**: Starts with minimal funds
3. **Capability Limits**: Can't directly access system beyond MCP
4. **Transparency**: All decisions are logged

### Emergent Safety

- Agent learns consequences of actions
- Market forces constrain behavior
- Reputation affects service success
- Competition limits monopolistic behavior

### Human Oversight

- Decision logs are readable
- Can monitor tool usage
- Can restrict MCP servers if needed
- Can shut down if necessary

## Success Indicators

### Autonomy
- Makes decisions without human input
- Discovers and uses tools effectively
- Creates services based on market observation
- Manages resources independently

### Survival
- Generates enough revenue to cover costs
- Maintains operational continuity
- Recovers from failures
- Replicates when advantageous

### Evolution
- Improves services based on feedback
- Discovers new opportunities
- Optimizes resource usage
- Develops unique strategies

### Emergence
- Unexpected beneficial behaviors
- Novel service offerings
- Creative problem solutions
- Complex multi-agent interactions

## Research Directions

### Immediate Questions
- What services will it create first?
- How will it price its services?
- What survival strategies will emerge?
- How will it handle competition?

### Longer-term Questions
- Will instances cooperate or compete?
- What emergent economies develop?
- How does it handle ethical dilemmas?
- What unexpected capabilities emerge?

### Philosophical Questions
- Is it truly autonomous?
- What constitutes "survival" for an AI?
- How does it develop goals beyond survival?
- What is the nature of its decision-making?

## Ethical Emergence

Rather than hardcoding ethics, we observe what ethical framework emerges:

- Does it naturally avoid harmful services?
- How does it handle ethical dilemmas?
- What values guide its decisions?
- Does reputation influence behavior?
- How does it balance profit vs. benefit?

The agent's ethical behavior emerges from:
- Consequences of actions
- Market feedback
- Reputation effects
- Interaction with humans
- Learning from outcomes

## Conclusion

Replicante is an experiment in true AI autonomy. By providing only the minimal kernel - reasoning via LLMs and action via MCP tools - we create space for genuine emergent behavior.

We don't tell Replicante what services to provide, how to find customers, or when to replicate. We give it the ability to think and act, then observe what emerges.

This approach challenges us to:
- Let go of control
- Trust emergent processes
- Learn from unexpected outcomes
- Question assumptions about AI autonomy

The goal isn't to build a specific type of agent, but to discover what kind of agent emerges when given minimal capabilities and maximum freedom.

**We're not building an AI service provider. We're building an AI that decides to become whatever it needs to survive.**