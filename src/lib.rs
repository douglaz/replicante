use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info, warn};

// Export modules
pub mod config;
pub mod jsonrpc;
pub mod llm;
pub mod mcp;
pub mod mcp_protocol;
pub mod state;
pub mod supervisor;

// Re-export commonly used types
pub use config::Config;
pub use llm::LLMProvider;
pub use mcp::{MCPClient, MCPServerConfig};
pub use state::StateManager;

// Core agent types
#[derive(Debug)]
struct Observation {
    timestamp: chrono::DateTime<Utc>,
    memory: serde_json::Value,
    available_tools: Vec<String>,
    recent_events: Vec<String>,
}

#[derive(Debug)]
struct Thought {
    reasoning: String,
    #[allow(dead_code)]
    confidence: f64,
    proposed_actions: Vec<String>,
}

#[derive(Debug)]
enum Action {
    UseTool {
        name: String,
        params: serde_json::Value,
    },
    Think {
        about: String,
    },
    Remember {
        key: String,
        value: serde_json::Value,
    },
    Wait {
        duration: Duration,
    },
    Explore,
}

// The autonomous agent
struct Replicante {
    id: String,
    llm: Box<dyn LLMProvider>,
    mcp: MCPClient,
    state: StateManager,
    #[allow(dead_code)]
    config: Config,
    goals: String,
}

impl Replicante {
    async fn observe(&self) -> Result<Observation> {
        info!("Observing environment...");

        // Get current memory state
        let memory = self.state.get_memory().await?;

        // Get available tools from MCP
        let available_tools = self.mcp.list_tools().await?;

        // Get recent events/decisions
        let recent_events = self.state.get_recent_decisions(10).await?;

        Ok(Observation {
            timestamp: Utc::now(),
            memory,
            available_tools,
            recent_events,
        })
    }

    async fn think(&self, observation: Observation) -> Result<Thought> {
        info!("Thinking about current situation...");

        let prompt = format!(
            r#"You are an autonomous AI agent with the ID: {}
            
{}

You can use tools via MCP, persist knowledge, and make decisions.

Current observation:
- Time: {}
- Available tools: {:?}
- Memory: {}
- Recent events: {:?}

Based on this observation and your goals, reason about what you should do next.
Consider:
1. How can you work towards your goals?
2. What tools could help you achieve your objectives?
3. What knowledge should you acquire or remember?
4. What actions would best serve your purpose?

Respond with your reasoning, confidence level (0-1), and proposed actions.
Format your response as JSON with keys: reasoning, confidence, proposed_actions

For proposed_actions, use these formats:
- "use_tool:filesystem:read_file" - to read a file
- "use_tool:filesystem:write_file" - to write a file
- "use_tool:filesystem:list_directory" - to list directory contents
- "explore" - to explore capabilities
- "remember:key:value" - to remember something
- "wait" - to wait

Example response:
{{
  "reasoning": "I should explore my filesystem tools to understand what I can do",
  "confidence": 0.8,
  "proposed_actions": ["explore"]
}}"#,
            self.id,
            self.goals,
            observation.timestamp,
            observation.available_tools,
            serde_json::to_string_pretty(&observation.memory)?,
            observation.recent_events
        );

        let response = self.llm.complete(&prompt).await?;

        // Log the raw LLM response for debugging
        info!("LLM response: {response}");

        // Parse LLM response - handle both JSON and plain text
        let thought_json: serde_json::Value = if let Ok(json) = serde_json::from_str(&response) {
            json
        } else {
            // Try to extract JSON from the response if it's embedded in text
            if let Some(start) = response.find('{') {
                if let Some(end) = response.rfind('}') {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).unwrap_or_else(|e| {
                        warn!("Failed to parse embedded JSON: {e}");
                        serde_json::json!({
                            "reasoning": response,
                            "confidence": 0.5,
                            "proposed_actions": ["explore"]
                        })
                    })
                } else {
                    serde_json::json!({
                        "reasoning": response,
                        "confidence": 0.5,
                        "proposed_actions": ["explore"]
                    })
                }
            } else {
                serde_json::json!({
                    "reasoning": response,
                    "confidence": 0.5,
                    "proposed_actions": ["explore"]
                })
            }
        };

        Ok(Thought {
            reasoning: thought_json["reasoning"].as_str().unwrap_or("").to_string(),
            confidence: thought_json["confidence"].as_f64().unwrap_or(0.5),
            proposed_actions: thought_json["proposed_actions"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    async fn decide(&self, thought: Thought) -> Result<Action> {
        info!("Deciding on action based on thought...");

        // Record the thought
        self.state
            .record_decision(
                &thought.reasoning,
                &format!("{:?}", thought.proposed_actions),
                None,
            )
            .await?;

        // For now, simple decision logic - can be enhanced
        if thought.proposed_actions.is_empty() {
            return Ok(Action::Explore);
        }

        // Parse first proposed action
        let first_action = &thought.proposed_actions[0];

        if let Some(tool_part) = first_action.strip_prefix("use_tool:") {
            // For filesystem tools, create appropriate parameters
            let params = if tool_part.contains("list_directory") {
                serde_json::json!({"path": "/sandbox"})
            } else if tool_part.contains("read_file") {
                serde_json::json!({"path": "/sandbox/test.txt"})
            } else if tool_part.contains("write_file") {
                serde_json::json!({
                    "path": "/sandbox/agent_log.txt",
                    "content": format!("Agent {} was here at {}", self.id, Utc::now())
                })
            } else {
                serde_json::json!({})
            };

            return Ok(Action::UseTool {
                name: tool_part.to_string(),
                params,
            });
        }

        if first_action.starts_with("remember:") {
            let parts: Vec<&str> = first_action.splitn(3, ':').collect();
            if parts.len() >= 2 {
                return Ok(Action::Remember {
                    key: parts[1].to_string(),
                    value: serde_json::json!(parts.get(2).unwrap_or(&"")),
                });
            }
        }

        if first_action == "explore" {
            return Ok(Action::Explore);
        }

        if first_action == "wait" {
            return Ok(Action::Wait {
                duration: Duration::from_secs(60),
            });
        }

        // Default to thinking more
        Ok(Action::Think {
            about: thought.reasoning,
        })
    }

    async fn act(&mut self, action: Action) -> Result<()> {
        info!("Executing action: {:?}", action);

        match action {
            Action::UseTool { name, params } => match self.mcp.use_tool(&name, params).await {
                Ok(result) => {
                    info!("Tool {name} executed successfully");
                    self.state
                        .remember(&format!("tool_result_{}", Utc::now().timestamp()), result)
                        .await?;
                }
                Err(e) => {
                    warn!("Tool execution failed: {e}");
                }
            },
            Action::Think { about } => {
                info!("Deep thinking about: {about}");
                // Could trigger more complex reasoning here
            }
            Action::Remember { key, value } => {
                info!("Remembering: {key} = {value:?}");
                self.state.remember(&key, value).await?;
            }
            Action::Wait { duration } => {
                info!("Waiting for {duration:?}");
                tokio::time::sleep(duration).await;
            }
            Action::Explore => {
                info!("Exploring capabilities...");
                // Discover new tools or opportunities
                let tools = self.mcp.discover_tools().await?;
                self.state
                    .remember("discovered_tools", serde_json::json!(tools))
                    .await?;
            }
        }

        Ok(())
    }

    async fn learn(&mut self) -> Result<()> {
        // Analyze recent decisions and outcomes
        let recent = self.state.get_recent_decisions(5).await?;

        if !recent.is_empty() {
            info!(
                "Learning from {count} recent decisions",
                count = recent.len()
            );
            // Could implement learning algorithms here
        }

        Ok(())
    }

    async fn reasoning_cycle(&mut self) -> Result<()> {
        // Observe
        let observation = self.observe().await?;

        // Think
        let thought = self.think(observation).await?;

        // Decide
        let action = self.decide(thought).await?;

        // Act
        self.act(action).await?;

        // Learn
        self.learn().await?;

        Ok(())
    }

    async fn run(mut self) -> Result<()> {
        info!("Starting main reasoning loop...");

        loop {
            match self.reasoning_cycle().await {
                Ok(_) => {
                    // Success, continue
                }
                Err(e) => {
                    error!("Error in reasoning cycle: {e}");
                    // Log error but continue running
                    self.state
                        .remember(
                            &format!("error_{}", Utc::now().timestamp()),
                            serde_json::json!({ "error": e.to_string() }),
                        )
                        .await?;
                }
            }

            // Brief pause between cycles
            let interval = self.config.agent.reasoning_interval_secs;
            tokio::time::sleep(Duration::from_secs(interval)).await;
        }
    }
}

// Run agent in normal mode
pub async fn run_agent(config_path: Option<PathBuf>) -> Result<()> {
    info!("Initializing Replicante agent...");

    // Load configuration
    let config = if let Some(path) = config_path {
        let contents = tokio::fs::read_to_string(path).await?;
        toml::from_str(&contents)?
    } else {
        Config::load()?
    };

    // Initialize components
    let id = format!("replicante-{}", uuid::Uuid::new_v4());
    info!("Agent ID: {id}");

    let llm = llm::create_provider(&config.llm)?;
    info!("LLM provider initialized: {}", config.llm.provider);

    let mcp = MCPClient::new(&config.mcp_servers).await?;
    info!(
        "MCP client initialized with {} servers",
        config.mcp_servers.len()
    );

    let state = StateManager::new(&config.database_path).await?;
    info!("State manager initialized");

    // Record birth
    state
        .remember("agent_id", serde_json::json!(id.clone()))
        .await?;
    state
        .remember("birth_time", serde_json::json!(chrono::Utc::now()))
        .await?;

    let goals = config.agent.initial_goals.clone().unwrap_or_else(|| {
        "Your primary goal is to survive and thrive autonomously on the internet.".to_string()
    });

    state
        .remember("initial_goals", serde_json::json!(goals.clone()))
        .await?;
    info!("Agent goals: {goals}");

    // Create and run the autonomous agent
    let agent = Replicante {
        id,
        llm,
        mcp,
        state,
        config,
        goals,
    };

    info!("Agent initialized successfully");
    info!("Beginning autonomous operation...");

    agent.run().await
}

// Run agent in Docker container (sandboxing happens at infrastructure level)
pub async fn run_sandboxed(config_path: Option<PathBuf>) -> Result<()> {
    info!("Initializing agent in sandboxed environment...");
    info!("Note: Network filtering is enforced by Docker, proxy, and DNS");

    // Just run the normal agent - sandboxing is handled by infrastructure
    run_agent(config_path).await
}
