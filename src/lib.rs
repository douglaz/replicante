use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::time::{Duration, Instant};
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

// Decision tracking types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub thought: String,
    pub action: String,
    pub parameters: Option<Value>,
    pub result: Option<DecisionResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionResult {
    pub status: String, // "success", "error", "timeout"
    pub summary: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
}

// Core agent types
#[derive(Debug)]
struct Observation {
    timestamp: chrono::DateTime<Utc>,
    memory: serde_json::Value,
    available_tools: Vec<String>,
    recent_events: Vec<DecisionRecord>,
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

        // Get summarized memory state (max 20 entries, max 10KB)
        let memory = self.state.get_memory_summary(20, 10_000).await?;

        // Get available tools from MCP
        let available_tools = self.mcp.list_tools().await?;

        // Get recent events/decisions as structured data (limit to 5 for context)
        let recent_events = self.state.get_recent_decisions_structured(5).await?;

        Ok(Observation {
            timestamp: Utc::now(),
            memory,
            available_tools,
            recent_events,
        })
    }

    fn generate_example_value(key: &str, schema: &serde_json::Value) -> String {
        let type_str = schema
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("string");

        match (key, type_str) {
            ("path", _) | ("file", _) | ("filename", _) => r#""progress.txt""#.to_string(),
            ("url", _) | ("uri", _) => r#""https://example.com""#.to_string(),
            ("content", _) | ("data", _) | ("text", _) => r#""Sample content here""#.to_string(),
            ("command", _) | ("cmd", _) => r#""ls -la""#.to_string(),
            ("directory", _) | ("dir", _) | ("folder", _) => r#""/workspace""#.to_string(),
            (_, "boolean") => "true".to_string(),
            (_, "number") | (_, "integer") => "42".to_string(),
            (_, "array") => r#"["item1", "item2"]"#.to_string(),
            (_, "object") => r#"{}"#.to_string(),
            _ => r#""example_value""#.to_string(),
        }
    }

    async fn generate_tool_examples(&self) -> Result<String> {
        info!("Generating tool examples from MCP schemas...");

        // Get tools with their full schemas
        let tools = self.mcp.get_tools_with_schemas().await?;
        info!("Found {} total tools", tools.len());

        let mut examples = Vec::new();

        // Prioritize tools that have been failing (especially write_file)
        let prioritized_tools: Vec<_> = tools
            .iter()
            .filter(|t| {
                t.name.contains("write_file")
                    || t.name.contains("http_get")
                    || t.name.contains("list_directory")
            })
            .chain(tools.iter())
            .take(3)
            .collect();

        info!(
            "Generating examples for {} prioritized tools",
            prioritized_tools.len()
        );

        for tool in prioritized_tools {
            if let Some(schema) = &tool.parameters {
                info!(
                    "Processing tool '{}' with schema: {}",
                    tool.name,
                    serde_json::to_string_pretty(schema).unwrap_or_else(|_| "invalid".to_string())
                );

                // Actually parse the schema JSON
                if let Some(properties) = schema.get("properties").and_then(|p| p.as_object()) {
                    let mut param_examples = Vec::new();

                    // Get required fields if specified
                    let required_fields = schema
                        .get("required")
                        .and_then(|r| r.as_array())
                        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
                        .unwrap_or_default();

                    info!(
                        "Tool '{}' has {} properties, {} required",
                        tool.name,
                        properties.len(),
                        required_fields.len()
                    );

                    // Generate example values for required parameters
                    for (key, prop_schema) in properties.iter() {
                        if required_fields.contains(&key.as_str()) || required_fields.is_empty() {
                            let example_value = Self::generate_example_value(key, prop_schema);
                            info!(
                                "  Parameter '{}': type={:?}, example={}",
                                key,
                                prop_schema.get("type"),
                                example_value
                            );
                            param_examples.push(format!(r#""{}": {}"#, key, example_value));

                            // Only show first 2-3 params for brevity
                            if param_examples.len() >= 2 {
                                break;
                            }
                        }
                    }

                    if !param_examples.is_empty() {
                        let action_desc = match tool.name.as_str() {
                            name if name.contains("write_file") => {
                                "I need to write content to a file"
                            }
                            name if name.contains("http_get") => {
                                "I need to fetch information from a URL"
                            }
                            name if name.contains("list_directory") => {
                                "I need to list directory contents"
                            }
                            name if name.contains("run_command") => {
                                "I need to execute a shell command"
                            }
                            _ => "I need to use this tool",
                        };

                        let example = format!(
                            r#"{{
  "reasoning": "{}.",
  "confidence": 0.9,
  "action": "use_tool:{}",
  "parameters": {{{}}}
}}"#,
                            action_desc,
                            tool.name,
                            param_examples.join(", ")
                        );

                        info!("Generated example for '{}': {}", tool.name, example);
                        examples.push(example);
                    } else {
                        warn!("No parameters generated for tool '{}'", tool.name);
                    }
                } else {
                    warn!("Tool '{}' has no properties in schema", tool.name);
                }
            } else {
                info!("Tool '{}' has no parameter schema", tool.name);
            }
        }

        if examples.is_empty() {
            warn!("No tool examples generated, using fallback");
            // Fallback to generic example
            examples.push(
                r#"{
  "reasoning": "I need to explore available tools.",
  "confidence": 0.8,
  "action": "explore"
}"#
                .to_string(),
            );
        } else {
            info!("Successfully generated {} tool examples", examples.len());
        }

        let result = examples.join("\n\n");
        info!("Final tool examples:\n{}", result);
        Ok(result)
    }

    async fn think(&self, observation: Observation) -> Result<Thought> {
        info!("Thinking about current situation...");

        // Generate tool format list from available tools (simplified for context)
        let tool_formats = observation
            .available_tools
            .iter()
            .take(10) // Limit to first 10 tools to save context
            .map(|tool| format!("- \"use_tool:{}\"", tool))
            .collect::<Vec<_>>()
            .join("\n");

        let tool_formats = if observation.available_tools.len() > 10 {
            format!(
                "{}\n... and {} more tools",
                tool_formats,
                observation.available_tools.len() - 10
            )
        } else {
            tool_formats
        };

        // Build action formats including discovered tools and built-in actions
        let action_formats = format!(
            r#"{}
- "remember:key" - persist knowledge (use parameters for value)
- "wait" - wait for a period of time
- "explore" - (deprecated - tools are auto-discovered)"#,
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

        // Generate dynamic examples based on actual tool schemas
        let tool_examples = self.generate_tool_examples().await.unwrap_or_else(|e| {
            error!("Failed to generate tool examples: {}", e);
            // Fallback to basic examples if generation fails
            r#"{
  "reasoning": "I need to take action.",
  "confidence": 0.8,
  "action": "explore"
}"#
            .to_string()
        });

        // Log the full prompt being sent to the LLM
        let prompt = format!(
            r#"You are an autonomous AI agent with the ID: {id}
            
{goals}

You can use tools via MCP, persist knowledge, and make decisions.

Current observation:
- Time: {timestamp}
- Available tools: {tools:?}
- Memory: {memory}
- Recent events:
{events}

IMPORTANT: You must make concrete progress toward your goals.
Take immediate action by:

Priority 1: Take concrete actions that create or change something
Priority 2: Gather specific information needed for the next action  
Priority 3: Use the tools available to make progress

Action Guidelines:
{guidelines}

Respond with your reasoning, confidence level (0-1), and a single action to take.
Format your response as JSON with keys: reasoning, confidence, action, parameters

Available action formats:
{action_formats}

IMPORTANT: Pay careful attention to the exact parameter names required for each tool.
Example responses showing correct parameter names:
{tool_examples}

Additional examples:
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
            events = serde_json::to_string_pretty(&observation.recent_events)?,
            guidelines = guidelines,
            action_formats = action_formats,
            tool_examples = tool_examples
        );

        // Log the complete prompt for debugging
        info!("=== SENDING PROMPT TO LLM ===");
        info!("Prompt length: {} characters", prompt.len());

        // Warn if context is too large
        if prompt.len() > 100_000 {
            warn!(
                "Prompt exceeds 100KB ({} chars) - may degrade LLM performance",
                prompt.len()
            );
        }
        if prompt.len() > 50_000 {
            info!(
                "Large prompt detected ({} chars) - consider further optimization",
                prompt.len()
            );
        }

        // Only log full prompt if it's reasonable size (for debugging)
        if prompt.len() < 50_000 {
            info!("Full prompt:\n{}", prompt);
        } else {
            info!(
                "Prompt too large to log fully. First 1000 chars:\n{}",
                &prompt[..1000.min(prompt.len())]
            );
        }
        info!("=== END OF PROMPT ===");

        let response = self.llm.complete(&prompt).await?;

        // Log the raw LLM response for debugging
        info!("=== LLM RESPONSE ===");
        info!("Response length: {} characters", response.len());
        info!("Full response: {}", response);
        info!("=== END OF RESPONSE ===");

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

    async fn decide(&self, thought: Thought) -> Result<(Action, i64)> {
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
                let decision_id = self
                    .state
                    .record_decision(
                        &format!("{} [learned]", thought.reasoning),
                        &format!(
                            "action: {}, params: {:?}",
                            thought.action, thought.parameters
                        ),
                        None,
                    )
                    .await?;

                let action = self.execute_decision(thought).await?;
                return Ok((action, decision_id));
            }
        }

        // Record the thought normally
        let decision_id = self
            .state
            .record_decision(
                &thought.reasoning,
                &format!(
                    "action: {}, params: {:?}",
                    thought.action, thought.parameters
                ),
                None,
            )
            .await?;

        let action = self.execute_decision(thought).await?;
        Ok((action, decision_id))
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

    async fn act(&mut self, action: Action, decision_id: i64) -> Result<()> {
        info!("Executing action: {:?}", action);
        let start_time = Instant::now();

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

                        // Store only a summary of large results
                        let result_str = serde_json::to_string(&result)?;
                        let result_to_store = if result_str.len() > 5000 {
                            // For large results, provide a useful summary
                            let truncated_content = if let Some(content) = result.get("content") {
                                // If there's a content field, truncate that specifically
                                content
                                    .as_str()
                                    .unwrap_or(&result_str)
                                    .chars()
                                    .take(2000)
                                    .collect::<String>()
                            } else {
                                // Otherwise truncate the whole result
                                result_str.chars().take(2000).collect::<String>()
                            };

                            serde_json::json!({
                                "success": true,
                                "tool": name,
                                "summary": format!("Large result truncated (original: {} bytes)", result_str.len()),
                                "truncated_content": truncated_content,
                                "timestamp": Utc::now()
                            })
                        } else {
                            result
                        };

                        self.state
                            .remember(
                                &format!(
                                    "tool_result_{timestamp}",
                                    timestamp = Utc::now().timestamp()
                                ),
                                result_to_store.clone(),
                            )
                            .await?;

                        // Update decision with success result
                        let duration_ms = start_time.elapsed().as_millis() as u64;
                        let result = DecisionResult {
                            status: "success".to_string(),
                            summary: Some(format!("Tool {} executed successfully", name)),
                            error: None,
                            duration_ms: Some(duration_ms),
                        };
                        self.state
                            .update_decision_result(decision_id, &result)
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

                        // Update decision with error result
                        let duration_ms = start_time.elapsed().as_millis() as u64;
                        let result = DecisionResult {
                            status: "error".to_string(),
                            summary: None,
                            error: Some(e.to_string()),
                            duration_ms: Some(duration_ms),
                        };
                        self.state
                            .update_decision_result(decision_id, &result)
                            .await?;
                    }
                }
            }
            Action::Remember { key, value } => {
                info!("Remembering: {key} = {value:?}");
                self.state.remember(&key, value.clone()).await?;

                // Update decision with success result
                let duration_ms = start_time.elapsed().as_millis() as u64;
                let result = DecisionResult {
                    status: "success".to_string(),
                    summary: Some(format!("Remembered key: {}", key)),
                    error: None,
                    duration_ms: Some(duration_ms),
                };
                self.state
                    .update_decision_result(decision_id, &result)
                    .await?;
            }
            Action::Wait { duration } => {
                info!("Waiting for {duration:?}");
                tokio::time::sleep(duration).await;

                // Update decision with success result
                let duration_ms = start_time.elapsed().as_millis() as u64;
                let result = DecisionResult {
                    status: "success".to_string(),
                    summary: Some(format!("Waited for {:?}", duration)),
                    error: None,
                    duration_ms: Some(duration_ms),
                };
                self.state
                    .update_decision_result(decision_id, &result)
                    .await?;
            }
            Action::Explore => {
                info!("Exploring capabilities...");
                // Tools are already discovered automatically in each observation
                // This action is now a no-op to avoid storing redundant data
                // Could be used for future exploration features

                // Update decision with success result
                let duration_ms = start_time.elapsed().as_millis() as u64;
                let result = DecisionResult {
                    status: "success".to_string(),
                    summary: Some("Explored capabilities".to_string()),
                    error: None,
                    duration_ms: Some(duration_ms),
                };
                self.state
                    .update_decision_result(decision_id, &result)
                    .await?;
            }
        }

        Ok(())
    }

    async fn learn(&mut self) -> Result<()> {
        // Analyze recent decisions and outcomes
        let recent = self.state.get_recent_decisions_structured(10).await?;

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
        let (action, decision_id) = self.decide(thought).await?;

        // Act
        self.act(action, decision_id).await?;

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

    #[test]
    fn test_generate_example_value() -> Result<()> {
        use serde_json::json;

        // Test path-like parameters
        assert_eq!(
            Replicante::generate_example_value("path", &json!({"type": "string"})),
            r#""progress.txt""#
        );
        assert_eq!(
            Replicante::generate_example_value("filename", &json!({"type": "string"})),
            r#""progress.txt""#
        );
        assert_eq!(
            Replicante::generate_example_value("file", &json!({"type": "string"})),
            r#""progress.txt""#
        );

        // Test URL parameters
        assert_eq!(
            Replicante::generate_example_value("url", &json!({"type": "string"})),
            r#""https://example.com""#
        );
        assert_eq!(
            Replicante::generate_example_value("uri", &json!({"type": "string"})),
            r#""https://example.com""#
        );

        // Test content parameters
        assert_eq!(
            Replicante::generate_example_value("content", &json!({"type": "string"})),
            r#""Sample content here""#
        );
        assert_eq!(
            Replicante::generate_example_value("data", &json!({"type": "string"})),
            r#""Sample content here""#
        );
        assert_eq!(
            Replicante::generate_example_value("text", &json!({"type": "string"})),
            r#""Sample content here""#
        );

        // Test command parameters
        assert_eq!(
            Replicante::generate_example_value("command", &json!({"type": "string"})),
            r#""ls -la""#
        );
        assert_eq!(
            Replicante::generate_example_value("cmd", &json!({"type": "string"})),
            r#""ls -la""#
        );

        // Test directory parameters
        assert_eq!(
            Replicante::generate_example_value("directory", &json!({"type": "string"})),
            r#""/workspace""#
        );
        assert_eq!(
            Replicante::generate_example_value("dir", &json!({"type": "string"})),
            r#""/workspace""#
        );
        assert_eq!(
            Replicante::generate_example_value("folder", &json!({"type": "string"})),
            r#""/workspace""#
        );

        // Test boolean type
        assert_eq!(
            Replicante::generate_example_value("enabled", &json!({"type": "boolean"})),
            "true"
        );
        assert_eq!(
            Replicante::generate_example_value("append", &json!({"type": "boolean"})),
            "true"
        );

        // Test number/integer type
        assert_eq!(
            Replicante::generate_example_value("count", &json!({"type": "integer"})),
            "42"
        );
        assert_eq!(
            Replicante::generate_example_value("size", &json!({"type": "number"})),
            "42"
        );

        // Test array type
        assert_eq!(
            Replicante::generate_example_value("items", &json!({"type": "array"})),
            r#"["item1", "item2"]"#
        );

        // Test object type
        assert_eq!(
            Replicante::generate_example_value("config", &json!({"type": "object"})),
            r#"{}"#
        );

        // Test unknown parameter with no type info
        assert_eq!(
            Replicante::generate_example_value("unknown", &json!({})),
            r#""example_value""#
        );

        Ok(())
    }

    #[test]
    fn test_schema_parameter_extraction() -> Result<()> {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "File path"},
                "content": {"type": "string", "description": "Content to write"},
                "append": {"type": "boolean", "default": false, "description": "Append mode"}
            },
            "required": ["path", "content"]
        });

        // Extract properties
        let properties = schema["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 3);
        assert!(properties.contains_key("path"));
        assert!(properties.contains_key("content"));
        assert!(properties.contains_key("append"));

        // Extract required fields
        let required = schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
        assert!(required.contains(&json!("path")));
        assert!(required.contains(&json!("content")));
        assert!(!required.contains(&json!("append")));

        // Check property types
        assert_eq!(properties["path"]["type"], "string");
        assert_eq!(properties["content"]["type"], "string");
        assert_eq!(properties["append"]["type"], "boolean");

        Ok(())
    }

    #[tokio::test]
    async fn test_decision_tracking_flow() -> Result<()> {
        use crate::state::StateManager;
        use tempfile::NamedTempFile;

        // Create temporary database
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file.path().to_str().unwrap();

        // Create state manager directly
        let state = StateManager::new(db_path).await?;

        // Simulate a decision
        let decision_id = state
            .record_decision("Test thought", "action: test_action, params: None", None)
            .await?;

        assert!(decision_id > 0, "Should return valid decision ID");

        // Update decision result
        let result = DecisionResult {
            status: "success".to_string(),
            summary: Some("Test completed".to_string()),
            error: None,
            duration_ms: Some(100),
        };

        state.update_decision_result(decision_id, &result).await?;

        // Verify decision was updated
        let decisions = state.get_recent_decisions_structured(1).await?;
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].id, decision_id);
        assert_eq!(decisions[0].result.as_ref().unwrap().status, "success");

        Ok(())
    }

    #[tokio::test]
    async fn test_tool_result_storage() -> Result<()> {
        use crate::state::StateManager;
        use chrono::Utc;
        use tempfile::NamedTempFile;

        // Create temporary database
        let temp_file = NamedTempFile::new()?;
        let db_path = temp_file.path().to_str().unwrap();

        // Create state manager directly
        let state = StateManager::new(db_path).await?;

        // Store a tool result
        let tool_result = json!({
            "tool": "test_tool",
            "success": true,
            "content": "Test content that should be visible",
            "timestamp": Utc::now().to_rfc3339(),
        });

        let key = format!(
            "tool_result_{}",
            Utc::now().timestamp_nanos_opt().unwrap_or(1)
        );
        state.remember(&key, tool_result.clone()).await?;

        // Get memory summary
        let summary = state.get_memory_summary(10, 10000).await?;
        let summary_obj = summary.as_object().unwrap();

        // Verify tool result is included
        assert!(
            summary_obj.contains_key(&key),
            "Tool result should be in memory summary"
        );

        let stored_result = summary_obj.get(&key).unwrap();
        assert_eq!(stored_result["tool"], "test_tool");
        assert_eq!(stored_result["success"], true);

        Ok(())
    }

    #[test]
    fn test_decision_record_serialization() -> Result<()> {
        use chrono::Utc;

        let record = DecisionRecord {
            id: 1,
            timestamp: Utc::now(),
            thought: "Test thought".to_string(),
            action: "test_action".to_string(),
            parameters: Some(json!({"key": "value"})),
            result: Some(DecisionResult {
                status: "success".to_string(),
                summary: Some("Completed".to_string()),
                error: None,
                duration_ms: Some(100),
            }),
        };

        // Serialize to JSON
        let json = serde_json::to_value(&record)?;

        // Verify fields are present
        assert_eq!(json["id"], 1);
        assert_eq!(json["thought"], "Test thought");
        assert_eq!(json["action"], "test_action");
        assert_eq!(json["parameters"]["key"], "value");
        assert_eq!(json["result"]["status"], "success");
        assert_eq!(json["result"]["duration_ms"], 100);

        // Deserialize back
        let deserialized: DecisionRecord = serde_json::from_value(json)?;
        assert_eq!(deserialized.id, record.id);
        assert_eq!(deserialized.thought, record.thought);
        assert_eq!(deserialized.result.unwrap().status, "success");

        Ok(())
    }

    #[test]
    fn test_decision_result_serialization() -> Result<()> {
        let result = DecisionResult {
            status: "error".to_string(),
            summary: None,
            error: Some("Connection failed".to_string()),
            duration_ms: Some(5000),
        };

        // Serialize to JSON
        let json = serde_json::to_value(&result)?;

        // Verify fields
        assert_eq!(json["status"], "error");
        assert_eq!(json["error"], "Connection failed");
        assert_eq!(json["duration_ms"], 5000);
        assert!(json["summary"].is_null());

        // Deserialize back
        let deserialized: DecisionResult = serde_json::from_value(json)?;
        assert_eq!(deserialized.status, "error");
        assert_eq!(deserialized.error, Some("Connection failed".to_string()));
        assert_eq!(deserialized.duration_ms, Some(5000));
        assert!(deserialized.summary.is_none());

        Ok(())
    }
}
