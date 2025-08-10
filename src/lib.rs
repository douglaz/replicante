use anyhow::Result;
use std::path::PathBuf;
use tracing::info;

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
pub use mcp::{MCPClient, MCPServerConfig};
pub use state::StateManager;

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

    let _llm = llm::create_provider(&config.llm)?;
    info!("LLM provider initialized: {}", config.llm.provider);

    let _mcp = MCPClient::new(&config.mcp_servers).await?;
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

    // Main agent loop would go here
    info!("Agent initialized successfully");
    info!("Beginning autonomous operation...");

    // For now, just run a simple loop
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        info!("Agent heartbeat");
    }
}

// Run agent in Docker container (sandboxing happens at infrastructure level)
pub async fn run_sandboxed(config_path: Option<PathBuf>) -> Result<()> {
    info!("Initializing agent in sandboxed environment...");
    info!("Note: Network filtering is enforced by Docker, proxy, and DNS");

    // Just run the normal agent - sandboxing is handled by infrastructure
    run_agent(config_path).await
}
