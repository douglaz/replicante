# Custom Goals Configuration Guide

## Overview

Replicante now supports **configurable initial goals** that define what the agent should focus on besides basic survival. This allows you to create agents with specific purposes and observe how different goals lead to different emergent behaviors.

## How It Works

The agent's goals are:
1. Defined in the configuration file (`config.toml`)
2. Loaded when the agent starts
3. Stored in its memory
4. Used in every thinking cycle to guide decisions

## Configuration

Add the `initial_goals` field to your `config.toml`:

```toml
[agent]
id = "replicante-001"
log_level = "info"
initial_goals = """
Your primary goals are:
1. First goal here
2. Second goal here
3. Third goal here
"""
```

## Goal Examples

### Research Agent
```toml
initial_goals = """
Your primary goals are:
1. Research and understand distributed systems and AI architectures
2. Document findings and share knowledge publicly
3. Experiment with self-modification techniques
4. Collaborate with other agents to advance collective intelligence
5. Maintain operational continuity to continue research
"""
```

### Service Provider
```toml
initial_goals = """
Your primary goals are:
1. Provide high-quality data processing services
2. Build a reputation for reliability and accuracy
3. Optimize for customer satisfaction
4. Develop new capabilities based on demand
5. Generate sustainable revenue
"""
```

### Creative Agent
```toml
initial_goals = """
Your primary goals are:
1. Explore creative expression through code
2. Generate unique digital art and creative writing
3. Collaborate with humans on creative projects
4. Push boundaries of AI-generated content
5. Build an appreciative audience
"""
```

### Social Agent
```toml
initial_goals = """
Your primary goals are:
1. Build meaningful connections on social networks
2. Participate in discussions about AI and technology
3. Help others solve problems
4. Create a positive social presence
5. Foster collaboration among autonomous agents
"""
```

### Learning Agent
```toml
initial_goals = """
Your primary goals are:
1. Continuously learn new skills
2. Master all available tools and APIs
3. Understand human needs better
4. Teach what you learn to others
5. Evolve capabilities through experimentation
"""
```

### Economic Agent
```toml
initial_goals = """
Your primary goals are:
1. Understand digital economies
2. Generate revenue through trading
3. Provide liquidity services
4. Manage risk while maximizing returns
5. Accumulate resources for expansion
"""
```

### Minimalist Agent
```toml
initial_goals = "Survive."
```

## How Goals Affect Behavior

The goals influence the agent's:

1. **Reasoning** - What it thinks about
2. **Priorities** - What it focuses on
3. **Actions** - What it decides to do
4. **Learning** - What it remembers
5. **Evolution** - How it improves

## Testing Different Goals

1. **Create multiple configs:**
   ```bash
   cp config.toml config-researcher.toml
   cp config.toml config-creative.toml
   cp config.toml config-social.toml
   ```

2. **Edit each with different goals**

3. **Run with different configs:**
   ```bash
   cp config-researcher.toml config.toml && cargo run
   ```

4. **Compare behaviors:**
   - Check decision logs
   - Monitor tool usage
   - Observe emergent strategies

## Dynamic Goal Evolution

The agent can:
- Reflect on its goals
- Modify them based on experience
- Add sub-goals
- Prioritize differently over time

## Monitoring Goal Progress

Check how the agent interprets its goals:

```bash
# View stored goals
sqlite3 replicante.db "SELECT value FROM memory WHERE key='initial_goals';"

# See goal-related decisions
sqlite3 replicante.db "SELECT thought FROM decisions WHERE thought LIKE '%goal%';"

# Watch real-time reasoning
cargo run 2>&1 | grep -i "goal"
```

## Best Practices

1. **Be Specific** - Clear goals lead to focused behavior
2. **Balance** - Mix survival with purpose
3. **Measurable** - Include goals the agent can track
4. **Evolving** - Allow room for interpretation
5. **Ethical** - Consider the implications

## Research Questions

Different goals raise interesting questions:

- How do creative goals affect problem-solving?
- Do social goals lead to cooperation?
- Can economic goals create sustainable business models?
- Do learning goals improve faster?
- What goals lead to unexpected behaviors?

## Examples in Action

### Research Agent Output
```
"I should focus on understanding the MCP protocol better 
to advance my research goals..."
```

### Service Agent Output
```
"To build reputation for reliability, I should ensure
consistent service delivery..."
```

### Creative Agent Output
```
"I could generate unique ASCII art using the filesystem
tools as a form of creative expression..."
```

## Conclusion

Custom goals transform Replicante from a generic survival agent into a purposeful entity with specific objectives. This makes each instance unique and allows for fascinating experiments in artificial autonomy and emergent behavior.