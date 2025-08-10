use anyhow::Result;
use replicante::mcp::{MCPClient, MCPServerConfig};
use std::path::PathBuf;
use std::time::Duration;
use tokio::time::timeout;

/// Helper to get the path to test files
fn test_path(file: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("test");
    path.push(file);
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
    }];

    // This should not hang - echo exits immediately
    let client = MCPClient::new(&configs).await?;
    assert_eq!(client.server_count(), 1);

    Ok(())
}

#[tokio::test]
#[ignore = "Python mock server integration pending fix for stdio buffering"]
async fn test_mock_server_full_flow() -> Result<()> {
    // Skip if Python is not available
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: Python3 not available");
        return Ok(());
    }

    let configs = vec![MCPServerConfig {
        name: "mock".to_string(),
        transport: "stdio".to_string(),
        command: "python3".to_string(),
        args: vec!["-u".to_string(), test_path("mock_mcp_server.py")],
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
#[ignore = "Python mock server integration pending fix for stdio buffering"]
async fn test_multiple_servers() -> Result<()> {
    // Skip if Python is not available
    if std::process::Command::new("python3")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("Skipping test: Python3 not available");
        return Ok(());
    }

    let configs = vec![
        MCPServerConfig {
            name: "mock1".to_string(),
            transport: "stdio".to_string(),
            command: "python3".to_string(),
            args: vec!["-u".to_string(), test_path("mock_mcp_server.py")],
        },
        MCPServerConfig {
            name: "mock2".to_string(),
            transport: "stdio".to_string(),
            command: "python3".to_string(),
            args: vec!["-u".to_string(), test_path("mock_mcp_server.py")],
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
async fn test_server_failure_recovery() -> Result<()> {
    let configs = vec![
        // This will fail
        MCPServerConfig {
            name: "failing".to_string(),
            transport: "stdio".to_string(),
            command: "nonexistent_command_xyz".to_string(),
            args: vec![],
        },
        // This should work if Python is available
        MCPServerConfig {
            name: "working".to_string(),
            transport: "stdio".to_string(),
            command: "echo".to_string(),
            args: vec!["test".to_string()],
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
