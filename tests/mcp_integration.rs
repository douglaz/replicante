use anyhow::Result;
use replicante::mcp::{MCPClient, MCPServerConfig};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

/// Helper to get the path to compiled binaries
fn target_binary_path(bin_name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("target");
    path.push("debug");
    path.push(bin_name);
    path.to_string_lossy().to_string()
}

#[tokio::test]
async fn test_echo_server() -> Result<()> {
    // Simple test with echo command that exits immediately
    let configs = vec![MCPServerConfig {
        name: "echo".to_string(),
        transport: "stdio".to_string(),
        command: "echo".to_string(),
        args: vec!["test".to_string()],
        retry_attempts: 1,
        retry_delay_ms: 100,
        health_check_interval_secs: 60,
    }];

    // This should not hang - echo exits immediately
    let client = MCPClient::new(&configs).await?;
    assert_eq!(client.server_count(), 1);

    Ok(())
}

#[tokio::test]
async fn test_mock_server_full_flow() -> Result<()> {
    let configs = vec![MCPServerConfig {
        name: "mock".to_string(),
        transport: "stdio".to_string(),
        command: target_binary_path("mock-mcp-server"),
        args: vec![],
        retry_attempts: 2,
        retry_delay_ms: 500,
        health_check_interval_secs: 30,
    }];

    // Create client with timeout
    let client = timeout(Duration::from_secs(5), MCPClient::new(&configs)).await??;

    // Wait a bit for initialization
    tokio::time::sleep(Duration::from_millis(500)).await;

    // List tools
    let tools = client.list_tools().await?;
    assert!(!tools.is_empty(), "Should have discovered tools");

    // Check for expected tools
    assert!(
        tools.contains(&"mock:echo".to_string()),
        "Should have echo tool"
    );
    assert!(
        tools.contains(&"mock:add".to_string()),
        "Should have add tool"
    );
    assert!(
        tools.contains(&"mock:get_time".to_string()),
        "Should have get_time tool"
    );

    // Test echo tool
    let result = client
        .use_tool(
            "mock:echo",
            serde_json::json!({
                "message": "Hello, MCP!"
            }),
        )
        .await?;

    assert!(result["success"].as_bool().unwrap_or(false));
    let content = result["content"].as_str().unwrap_or("");
    assert!(
        content.contains("Hello, MCP!"),
        "Echo should return our message"
    );

    // Test add tool
    let result = client
        .use_tool(
            "mock:add",
            serde_json::json!({
                "a": 5,
                "b": 3
            }),
        )
        .await?;

    assert!(result["success"].as_bool().unwrap_or(false));
    let content = result["content"].as_str().unwrap_or("");
    assert!(content.contains("8"), "Add should return 5 + 3 = 8");

    // Test get_time tool
    let result = client
        .use_tool("mock:get_time", serde_json::json!({}))
        .await?;

    assert!(result["success"].as_bool().unwrap_or(false));
    let content = result["content"].as_str().unwrap_or("");
    assert!(
        content.contains("Current time"),
        "Should return current time"
    );

    Ok(())
}

#[tokio::test]
async fn test_multiple_servers() -> Result<()> {
    let configs = vec![
        MCPServerConfig {
            name: "mock1".to_string(),
            transport: "stdio".to_string(),
            command: target_binary_path("mock-mcp-server"),
            args: vec![],
            retry_attempts: 2,
            retry_delay_ms: 500,
            health_check_interval_secs: 30,
        },
        MCPServerConfig {
            name: "mock2".to_string(),
            transport: "stdio".to_string(),
            command: target_binary_path("mock-mcp-server"),
            args: vec![],
            retry_attempts: 2,
            retry_delay_ms: 500,
            health_check_interval_secs: 30,
        },
    ];

    let client = timeout(Duration::from_secs(5), MCPClient::new(&configs)).await??;

    // Wait for initialization
    tokio::time::sleep(Duration::from_millis(500)).await;

    let tools = client.list_tools().await?;

    // Should have tools from both servers
    assert!(tools.contains(&"mock1:echo".to_string()));
    assert!(tools.contains(&"mock2:echo".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_http_mcp_server() -> Result<()> {
    let configs = vec![MCPServerConfig {
        name: "http".to_string(),
        transport: "stdio".to_string(),
        command: target_binary_path("http-mcp-server"),
        args: vec![],
        retry_attempts: 2,
        retry_delay_ms: 500,
        health_check_interval_secs: 30,
    }];

    // Create client with timeout
    let client = timeout(Duration::from_secs(10), MCPClient::new(&configs)).await??;

    // Wait a bit for initialization
    tokio::time::sleep(Duration::from_millis(500)).await;

    // List tools
    let tools = client.list_tools().await?;
    assert!(!tools.is_empty(), "Should have discovered tools");

    // Check for expected tools
    assert!(
        tools.contains(&"http:fetch_url".to_string()),
        "Should have fetch_url tool"
    );
    assert!(
        tools.contains(&"http:check_weather".to_string()),
        "Should have check_weather tool"
    );
    assert!(
        tools.contains(&"http:get_time".to_string()),
        "Should have get_time tool"
    );
    assert!(
        tools.contains(&"http:calculate".to_string()),
        "Should have calculate tool"
    );

    // Test calculate tool
    let result = client
        .use_tool(
            "http:calculate",
            serde_json::json!({
                "expression": "15 + 25 * 2"
            }),
        )
        .await?;

    assert!(result["success"].as_bool().unwrap_or(false));
    let content = result["content"].as_str().unwrap_or("");
    assert!(
        content.contains("65"),
        "Calculate should return 15 + 25 * 2 = 65"
    );

    // Test weather tool
    let result = client
        .use_tool(
            "http:check_weather",
            serde_json::json!({
                "city": "London"
            }),
        )
        .await?;

    assert!(result["success"].as_bool().unwrap_or(false));
    let content = result["content"].as_str().unwrap_or("");
    assert!(
        content.contains("London"),
        "Weather should mention the city"
    );

    // Test time tool
    let result = client
        .use_tool(
            "http:get_time",
            serde_json::json!({
                "timezone": "UTC"
            }),
        )
        .await?;

    assert!(result["success"].as_bool().unwrap_or(false));
    let content = result["content"].as_str().unwrap_or("");
    assert!(content.contains("UTC"), "Time should include timezone info");

    Ok(())
}

#[tokio::test]
async fn test_server_failure_recovery() -> Result<()> {
    let configs = vec![
        // This will fail
        MCPServerConfig {
            name: "failing".to_string(),
            transport: "stdio".to_string(),
            command: "nonexistent_command_xyz".to_string(),
            args: vec![],
            retry_attempts: 1,
            retry_delay_ms: 100,
            health_check_interval_secs: 60,
        },
        // This should work if Python is available
        MCPServerConfig {
            name: "working".to_string(),
            transport: "stdio".to_string(),
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            retry_attempts: 1,
            retry_delay_ms: 100,
            health_check_interval_secs: 60,
        },
    ];

    // Should not panic even with one failing server
    let client = MCPClient::new(&configs).await?;

    // Should have both servers in the list (even if one failed)
    assert_eq!(client.server_count(), 2);

    Ok(())
}

#[tokio::test]
async fn test_invalid_tool_name() -> Result<()> {
    let configs = vec![];
    let client = MCPClient::new(&configs).await?;

    // Test invalid tool name format
    let result = client
        .use_tool("invalid_format", serde_json::json!({}))
        .await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid tool name format")
    );

    // Test non-existent server
    let result = client
        .use_tool("nonexistent:tool", serde_json::json!({}))
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Server not found"));

    Ok(())
}
