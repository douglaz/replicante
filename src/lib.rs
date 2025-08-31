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
    action: String,
    parameters: Option<serde_json::Value>,
}

#[derive(Debug)]
enum Action {
    UseTool {
        name: String,
        params: serde_json::Value,
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

        // Generate tool format list from available tools
        let tool_formats = observation
            .available_tools
            .iter()
            .map(|tool| format!("- \"use_tool:{}\" - use the {} tool", tool, tool))
            .collect::<Vec<_>>()
            .join("\n");

        // Build action formats including discovered tools and built-in actions
        let action_formats = format!(
            r#"{}
- "explore" - discover available tools (use sparingly)
- "remember:key" - persist knowledge (use parameters for value)
- "wait" - wait for a period of time"#,
            tool_formats
        );

        // Generate action guidelines based on available tools
        let mut action_guidelines = Vec::new();
        for tool in &observation.available_tools {
            match tool.as_str() {
                "http:http_get" => action_guidelines.push("- If you need to research something, use http:http_get to search for information"),
                "shell:docker_pull" | "http:download_file" => {
                    if tool == "shell:docker_pull" {
                        action_guidelines.push("- If you need software, use shell:docker_pull to obtain Docker images");
                    } else {
                        action_guidelines.push("- If you need software, use http:download_file to obtain it");
                    }
                },
                "filesystem:write_file" => action_guidelines.push("- Create files with filesystem:write_file to save your progress and configurations"),
                "shell:run_command" => action_guidelines.push("- Use shell:run_command to execute commands and scripts"),
                "shell:docker_run" => action_guidelines.push("- Use shell:docker_run to start containers"),
                _ => {}
            }
        }
        let guidelines = if action_guidelines.is_empty() {
            "- Use the available tools to make progress toward your goals".to_string()
        } else {
            action_guidelines.join("\n")
        };

        let prompt = format!(
            r#"You are an autonomous AI agent with the ID: {id}
            
{goals}

You can use tools via MCP, persist knowledge, and make decisions.

Current observation:
- Time: {timestamp}
- Available tools: {tools:?}
- Memory: {memory}
- Recent events: {events:?}

IMPORTANT: You must make concrete progress toward your goals. Exploration alone is not progress.
Take immediate action by:

Priority 1: Take concrete actions that create or change something
Priority 2: Gather specific information needed for the next action  
Priority 3: Only explore when you have no other options

Action Guidelines:
{guidelines}

Respond with your reasoning, confidence level (0-1), and a single action to take.
Format your response as JSON with keys: reasoning, confidence, action, parameters

Available action formats:
{action_formats}

Example responses:
{{
  "reasoning": "I need to fetch information from a website to understand the topic better.",
  "confidence": 0.9,
  "action": "use_tool:http:http_get",
  "parameters": {{"url": "https://example.com"}}
}}

{{
  "reasoning": "I should explore what tools are available to me.",
  "confidence": 0.8,
  "action": "explore"
}}

{{
  "reasoning": "I need to save this important information for later use.",
  "confidence": 0.95,
  "action": "remember:key_name",
  "parameters": {{"value": "important data"}}
}}"#,
            id = self.id,
            goals = self.goals,
            timestamp = observation.timestamp,
            tools = observation.available_tools,
            memory = serde_json::to_string_pretty(&observation.memory)?,
            events = observation.recent_events,
            guidelines = guidelines,
            action_formats = action_formats
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
                            "action": "explore"
                        })
                    })
                } else {
                    serde_json::json!({
                        "reasoning": response,
                        "confidence": 0.5,
                        "action": "explore"
                    })
                }
            } else {
                serde_json::json!({
                    "reasoning": response,
                    "confidence": 0.5,
                    "action": "explore"
                })
            }
        };

        Ok(Thought {
            reasoning: thought_json["reasoning"].as_str().unwrap_or("").to_string(),
            confidence: thought_json["confidence"].as_f64().unwrap_or(0.5),
            action: thought_json["action"]
                .as_str()
                .unwrap_or("wait")
                .to_string(),
            parameters: thought_json.get("parameters").cloned(),
        })
    }

    async fn decide(&self, thought: Thought) -> Result<Action> {
        info!("Deciding on action based on thought...");

        // Check if we have learned patterns for this context
        let context = thought.reasoning.chars().take(100).collect::<String>();
        if let Ok(Some((best_action, confidence))) = self
            .state
            .get_best_action_for_context("reasoning", &context, 0.7)
            .await
        {
            info!("Found learned pattern with {confidence:.2} confidence: {best_action}");
            // Consider using the learned action if confidence is high enough
            if confidence > 0.85 && thought.action == best_action {
                info!("Using learned action based on past success");

                // Record the decision with learning influence
                self.state
                    .record_decision(
                        &format!("{} [learned]", thought.reasoning),
                        &format!(
                            "action: {}, params: {:?}",
                            thought.action, thought.parameters
                        ),
                        None,
                    )
                    .await?;

                return self.execute_decision(thought).await;
            }
        }

        // Record the thought normally
        self.state
            .record_decision(
                &thought.reasoning,
                &format!(
                    "action: {}, params: {:?}",
                    thought.action, thought.parameters
                ),
                None,
            )
            .await?;

        self.execute_decision(thought).await
    }

    async fn execute_decision(&self, thought: Thought) -> Result<Action> {
        // Parse the single action
        if thought.action.is_empty() {
            return Ok(Action::Explore);
        }

        if let Some(tool_part) = thought.action.strip_prefix("use_tool:") {
            // tool_part is like "filesystem:read_file" - the full MCP tool name
            // Use provided parameters or default to empty object
            let params = thought.parameters.unwrap_or_else(|| serde_json::json!({}));

            return Ok(Action::UseTool {
                name: tool_part.to_string(),
                params,
            });
        }

        if thought.action.starts_with("remember:") {
            let key = thought
                .action
                .strip_prefix("remember:")
                .unwrap_or("memory")
                .to_string();
            let value = thought.parameters.unwrap_or_else(|| serde_json::json!(""));

            return Ok(Action::Remember { key, value });
        }

        if thought.action == "explore" {
            return Ok(Action::Explore);
        }

        if thought.action == "wait" {
            return Ok(Action::Wait {
                duration: Duration::from_secs(60),
            });
        }

        // Invalid action format - return error so agent can see and correct
        anyhow::bail!(
            "Invalid action format: '{}'. Expected one of: use_tool:<tool>, remember:<key>, explore, wait",
            thought.action
        )
    }

    async fn act(&mut self, action: Action) -> Result<()> {
        info!("Executing action: {:?}", action);

        match action {
            Action::UseTool { name, params } => {
                let context = format!("tool_use_{name}");
                match self.mcp.use_tool(&name, params.clone()).await {
                    Ok(result) => {
                        info!("Tool {name} executed successfully");

                        // Record successful pattern
                        self.state
                            .record_action_pattern(
                                "tool_execution",
                                &context,
                                &name,
                                Some(&format!("{result:?}")),
                                true,
                            )
                            .await?;

                        // Update capability tracking
                        self.state.record_capability(&name, None, true).await?;

                        self.state
                            .remember(
                                &format!(
                                    "tool_result_{timestamp}",
                                    timestamp = Utc::now().timestamp()
                                ),
                                result,
                            )
                            .await?;
                    }
                    Err(e) => {
                        warn!("Tool execution failed: {e}");

                        // Store failure in memory so agent can observe it
                        self.state
                            .remember(
                                &format!("tool_result_{}", Utc::now().timestamp()),
                                serde_json::json!({
                                    "success": false,
                                    "tool": name,
                                    "error": e.to_string(),
                                    "timestamp": Utc::now()
                                }),
                            )
                            .await?;

                        // Record failure pattern
                        self.state
                            .record_action_pattern(
                                "tool_execution",
                                &context,
                                &name,
                                Some(&e.to_string()),
                                false,
                            )
                            .await?;

                        // Update capability tracking
                        self.state.record_capability(&name, None, false).await?;
                    }
                }
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
        let recent = self.state.get_recent_decisions(10).await?;

        if !recent.is_empty() {
            info!(
                "Learning from {count} recent decisions",
                count = recent.len()
            );

            // Analyze patterns in recent decisions
            let analysis = self.state.analyze_decision_patterns(24).await?;

            // Update learning metrics based on analysis
            if let Some(patterns) = analysis["successful_patterns"].as_array() {
                let success_rate = patterns.len() as f64 / recent.len() as f64;
                self.state
                    .update_learning_metric("decision_success_rate", success_rate)
                    .await?;
            }

            // Track tool performance
            if let Some(tools) = analysis["tool_performance"].as_array() {
                for tool in tools {
                    if let (Some(name), Some(rate)) =
                        (tool["tool"].as_str(), tool["success_rate"].as_f64())
                    {
                        self.state
                            .update_learning_metric(&format!("tool_success_{name}"), rate)
                            .await?;
                    }
                }
            }

            // Store learning insights
            self.state
                .remember("last_learning_analysis", analysis)
                .await?;
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
                            &format!("error_{timestamp}", timestamp = Utc::now().timestamp()),
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
    let id = format!("replicante-{uuid}", uuid = uuid::Uuid::new_v4());
    info!("Agent ID: {id}");

    let llm = llm::create_provider(&config.llm)?;
    info!(
        "LLM provider initialized: {provider}",
        provider = config.llm.provider
    );

    let mcp = MCPClient::new(&config.mcp_servers).await?;
    info!(
        "MCP client initialized with {count} servers",
        count = config.mcp_servers.len()
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Helper function to create a test Replicante instance
    fn create_test_agent() -> Replicante {
        Replicante {
            id: "test-agent".to_string(),
            llm: Box::new(llm::MockLLMProvider::new()),
            mcp: futures::executor::block_on(MCPClient::new(&[])).unwrap(),
            state: futures::executor::block_on(StateManager::new(":memory:")).unwrap(),
            config: Config::default(),
            goals: "Test goals".to_string(),
        }
    }

    // Helper to parse thought from JSON string
    fn parse_thought_json(json_str: &str) -> Result<Thought> {
        let thought_json: serde_json::Value = serde_json::from_str(json_str)?;
        Ok(Thought {
            reasoning: thought_json["reasoning"].as_str().unwrap_or("").to_string(),
            confidence: thought_json["confidence"].as_f64().unwrap_or(0.5),
            action: thought_json["action"]
                .as_str()
                .unwrap_or("wait")
                .to_string(),
            parameters: thought_json.get("parameters").cloned(),
        })
    }

    #[test]
    fn test_parse_thought_with_tool_and_params() -> Result<()> {
        let json_response = r#"{
            "reasoning": "I need to fetch data from the API",
            "confidence": 0.9,
            "action": "use_tool:http:http_get",
            "parameters": {"url": "https://api.example.com"}
        }"#;

        let thought = parse_thought_json(json_response)?;
        assert_eq!(thought.action, "use_tool:http:http_get");
        assert_eq!(
            thought.parameters.unwrap()["url"],
            "https://api.example.com"
        );
        assert_eq!(thought.confidence, 0.9);
        Ok(())
    }

    #[test]
    fn test_parse_thought_without_params() -> Result<()> {
        let json_response = r#"{
            "reasoning": "I should explore available tools",
            "confidence": 0.8,
            "action": "explore"
        }"#;

        let thought = parse_thought_json(json_response)?;
        assert_eq!(thought.action, "explore");
        assert!(thought.parameters.is_none());
        assert_eq!(thought.confidence, 0.8);
        Ok(())
    }

    #[test]
    fn test_parse_thought_remember_action() -> Result<()> {
        let json_response = r#"{
            "reasoning": "I need to remember this information",
            "confidence": 0.95,
            "action": "remember:important_fact",
            "parameters": {"value": "Fedimint is a federated e-cash system"}
        }"#;

        let thought = parse_thought_json(json_response)?;
        assert_eq!(thought.action, "remember:important_fact");
        assert!(thought.parameters.is_some());
        assert_eq!(
            thought.parameters.unwrap()["value"],
            "Fedimint is a federated e-cash system"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_decision_tool_with_params() -> Result<()> {
        let agent = create_test_agent();
        let thought = Thought {
            reasoning: "Testing tool execution".to_string(),
            confidence: 0.9,
            action: "use_tool:filesystem:read_file".to_string(),
            parameters: Some(json!({"path": "test.txt"})),
        };

        let action = agent.execute_decision(thought).await?;
        match action {
            Action::UseTool { name, params } => {
                assert_eq!(name, "filesystem:read_file");
                assert_eq!(params["path"], "test.txt");
            }
            _ => panic!("Expected UseTool action"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_decision_empty_params() -> Result<()> {
        let agent = create_test_agent();
        let thought = Thought {
            reasoning: "Testing tool without params".to_string(),
            confidence: 0.9,
            action: "use_tool:filesystem:list_directory".to_string(),
            parameters: None,
        };

        let action = agent.execute_decision(thought).await?;
        match action {
            Action::UseTool { name, params } => {
                assert_eq!(name, "filesystem:list_directory");
                assert_eq!(params, json!({}));
            }
            _ => panic!("Expected UseTool action"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_decision_remember() -> Result<()> {
        let agent = create_test_agent();
        let thought = Thought {
            reasoning: "Testing remember action".to_string(),
            confidence: 0.9,
            action: "remember:test_key".to_string(),
            parameters: Some(json!({"data": "test_value"})),
        };

        let action = agent.execute_decision(thought).await?;
        match action {
            Action::Remember { key, value } => {
                assert_eq!(key, "test_key");
                assert_eq!(value, json!({"data": "test_value"}));
            }
            _ => panic!("Expected Remember action"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_decision_explore() -> Result<()> {
        let agent = create_test_agent();
        let thought = Thought {
            reasoning: "Testing explore".to_string(),
            confidence: 0.9,
            action: "explore".to_string(),
            parameters: None,
        };

        let action = agent.execute_decision(thought).await?;
        assert!(matches!(action, Action::Explore));
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_decision_wait() -> Result<()> {
        let agent = create_test_agent();
        let thought = Thought {
            reasoning: "Testing wait".to_string(),
            confidence: 0.9,
            action: "wait".to_string(),
            parameters: None,
        };

        let action = agent.execute_decision(thought).await?;
        assert!(matches!(action, Action::Wait { .. }));
        Ok(())
    }

    #[test]
    fn test_malformed_json_fallback() -> Result<()> {
        // Test with missing action field
        let json_response = r#"{
            "reasoning": "Missing action field",
            "confidence": 0.5
        }"#;

        let thought = parse_thought_json(json_response)?;
        assert_eq!(thought.action, "wait"); // Should default to "wait"
        assert_eq!(thought.confidence, 0.5);

        Ok(())
    }

    #[test]
    fn test_parse_thought_with_filesystem_tool() -> Result<()> {
        let json_response = r#"{
            "reasoning": "I need to list the workspace directory",
            "confidence": 0.85,
            "action": "use_tool:filesystem:list_directory",
            "parameters": {"path": "/workspace", "recursive": false}
        }"#;

        let thought = parse_thought_json(json_response)?;
        assert_eq!(thought.action, "use_tool:filesystem:list_directory");
        let params = thought.parameters.unwrap();
        assert_eq!(params["path"], "/workspace");
        assert_eq!(params["recursive"], false);
        Ok(())
    }

    #[test]
    fn test_parse_thought_with_docker_tool() -> Result<()> {
        let json_response = r#"{
            "reasoning": "I need to pull the Fedimint Docker image",
            "confidence": 0.92,
            "action": "use_tool:shell:docker_pull",
            "parameters": {"image": "fedimint/fedimint:latest"}
        }"#;

        let thought = parse_thought_json(json_response)?;
        assert_eq!(thought.action, "use_tool:shell:docker_pull");
        assert_eq!(
            thought.parameters.unwrap()["image"],
            "fedimint/fedimint:latest"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_decision_invalid_action_returns_error() -> Result<()> {
        let agent = create_test_agent();

        // Test with missing use_tool: prefix
        let thought = Thought {
            reasoning: "Testing invalid action format".to_string(),
            confidence: 0.9,
            action: "shell:run_command".to_string(),
            parameters: Some(json!({"command": "ls"})),
        };

        let result = agent.execute_decision(thought).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid action format"));
        assert!(err.to_string().contains("shell:run_command"));

        // Test with completely unknown action
        let thought2 = Thought {
            reasoning: "Testing unknown action".to_string(),
            confidence: 0.9,
            action: "unknown_action".to_string(),
            parameters: None,
        };

        let result2 = agent.execute_decision(thought2).await;
        assert!(result2.is_err());
        let err2 = result2.unwrap_err();
        assert!(err2.to_string().contains("Invalid action format"));
        assert!(err2.to_string().contains("unknown_action"));

        Ok(())
    }
}
