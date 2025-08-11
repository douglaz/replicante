use anyhow::Result;
use replicante::{StateManager, run_agent};
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;

/// Test configuration for autonomous reasoning
fn create_test_config(db_path: &str) -> replicante::Config {
    replicante::Config {
        database_path: db_path.to_string(),
        agent: replicante::config::AgentConfig {
            id: Some("test-agent".to_string()),
            log_level: Some("debug".to_string()),
            initial_goals: Some("Test autonomous reasoning cycle".to_string()),
            reasoning_interval_secs: 1,
        },
        llm: replicante::llm::LLMConfig {
            provider: "mock".to_string(),
            api_key: None,
            model: "mock".to_string(),
            temperature: None,
            max_tokens: None,
            api_url: None,
        },
        mcp_servers: vec![],
    }
}

#[tokio::test]
async fn test_autonomous_reasoning_cycle_exists() -> Result<()> {
    // This test verifies that all components of the autonomous reasoning cycle exist
    // and can be instantiated. It will fail at compile time if any component is missing.

    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test.db");
    let config = create_test_config(db_path.to_str().unwrap());

    // Create components
    let llm = replicante::llm::create_provider(&config.llm)?;
    let state = StateManager::new(&config.database_path).await?;
    let mcp = replicante::mcp::MCPClient::new(&config.mcp_servers).await?;

    // Verify we can create all components
    assert!(llm.complete("test").await.is_ok());
    assert!(state.get_memory().await.is_ok());
    assert_eq!(mcp.server_count(), 0); // No servers configured

    Ok(())
}

#[tokio::test]
async fn test_agent_with_mock_llm() -> Result<()> {
    // Test that the agent can run with a mock LLM provider
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("test_agent.db");

    // Create config file
    let config_path = temp_dir.path().join("config.toml");
    let config_content = format!(
        r#"
database_path = "{}"
mcp_servers = []

[agent]
id = "test-reasoning-agent"
log_level = "info"
initial_goals = "Test the complete autonomous reasoning cycle"

[llm]
provider = "mock"
model = "mock"
"#,
        db_path.display()
    );

    tokio::fs::write(&config_path, config_content).await?;

    // Run the agent for a short time to verify it starts and operates
    let config_path_clone = config_path.clone();
    let agent_handle = tokio::spawn(async move {
        match run_agent(Some(config_path_clone)).await {
            Ok(_) => {
                // Agent exited normally (shouldn't happen with infinite loop)
                eprintln!("Agent exited normally");
            }
            Err(e) => {
                // Agent exited with error
                eprintln!("Agent error: {e}");
            }
        }
    });

    // Let it run for 2 seconds
    tokio::time::sleep(Duration::from_secs(2)).await;

    // Agent should still be running (not crashed)
    assert!(!agent_handle.is_finished(), "Agent should still be running");

    // Clean up by dropping the handle (will cancel the task)
    drop(agent_handle);

    // Verify the database was created and has some data
    assert!(db_path.exists());

    // Check that state was persisted
    let state = StateManager::new(db_path.to_str().unwrap()).await?;
    let memory = state.get_memory().await?;

    // Agent should have stored initial data
    assert!(memory.get("agent_id").is_some());
    assert!(memory.get("birth_time").is_some());
    assert!(memory.get("initial_goals").is_some());

    Ok(())
}

#[tokio::test]
async fn test_reasoning_methods_execution() -> Result<()> {
    // This test verifies that the reasoning cycle methods can be executed
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("reasoning_test.db");
    let config = create_test_config(db_path.to_str().unwrap());

    // Create components
    let llm = replicante::llm::create_provider(&config.llm)?;
    let state = StateManager::new(&config.database_path).await?;

    // Test LLM mock responses
    let response1 = llm.complete("test observation").await?;
    assert!(response1.contains("reasoning"));
    assert!(response1.contains("proposed_actions"));

    // Test state persistence
    state
        .remember("test_key", serde_json::json!("test_value"))
        .await?;
    let memory = state.get_memory().await?;
    assert_eq!(
        memory.get("test_key"),
        Some(&serde_json::json!("test_value"))
    );

    // Test decision recording
    state
        .record_decision("test thought", "test action", Some("test result"))
        .await?;
    let decisions = state.get_recent_decisions(1).await?;
    assert_eq!(decisions.len(), 1);
    assert!(decisions[0].contains("test thought"));

    Ok(())
}

#[tokio::test]
async fn test_agent_makes_decisions() -> Result<()> {
    // Test that the agent actually makes and records decisions
    let temp_dir = TempDir::new()?;
    let db_path = temp_dir.path().join("decisions_test.db");

    // Create config file
    let config_path = temp_dir.path().join("config.toml");
    let config_content = format!(
        r#"
database_path = "{}"
mcp_servers = []

[agent]
id = "decision-test-agent"
log_level = "info"
initial_goals = "Make decisions and record them"
reasoning_interval_secs = 1

[llm]
provider = "mock"
model = "mock"
"#,
        db_path.display()
    );

    tokio::fs::write(&config_path, config_content).await?;

    // Run the agent
    let config_path_clone = config_path.clone();
    let agent_handle = tokio::spawn(async move {
        match run_agent(Some(config_path_clone)).await {
            Ok(_) => {
                eprintln!("Agent exited normally");
            }
            Err(e) => {
                eprintln!("Agent error: {e}");
            }
        }
    });

    // Let it run for enough time to make several decisions (with 1 second interval)
    tokio::time::sleep(Duration::from_secs(4)).await;

    // Clean up
    drop(agent_handle);

    // Check decisions were made
    let state = StateManager::new(db_path.to_str().unwrap()).await?;
    let decisions = state.get_recent_decisions(10).await?;

    // Should have made at least 2 decisions in 4 seconds with 1 second interval
    assert!(
        decisions.len() >= 2,
        "Agent should have made multiple decisions, got: {count}",
        count = decisions.len()
    );

    // Verify decisions contain expected content
    let all_decisions = decisions.join(" ");
    assert!(
        all_decisions.contains("explore")
            || all_decisions.contains("use_tool")
            || all_decisions.contains("remember")
    );

    Ok(())
}

/// This test ensures that the core Replicante struct and its methods exist.
/// It will fail at compile time if the autonomous reasoning implementation is removed.
#[test]
fn test_core_components_exist() {
    // This function doesn't need to run, it just needs to compile
    // If any of these types or methods don't exist, compilation will fail

    fn _type_check() {
        // These lines verify that the types exist at compile time
        let _: Option<replicante::Config> = None;
        let _: Option<Box<dyn replicante::LLMProvider>> = None;
        let _: Option<replicante::StateManager> = None;
        let _: Option<replicante::mcp::MCPClient> = None;

        // Verify public functions exist
        let _: fn(Option<PathBuf>) -> _ = replicante::run_agent;
        let _: fn(Option<PathBuf>) -> _ = replicante::run_sandboxed;
    }
}
