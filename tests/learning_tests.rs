use anyhow::Result;
use replicante::state::StateManager;
use tempfile::NamedTempFile;

#[tokio::test]
async fn test_action_pattern_learning() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let db_path = temp_file.path().to_str().unwrap();

    let state = StateManager::new(db_path).await?;

    // Record some action patterns
    state
        .record_action_pattern(
            "tool_usage",
            "fetch_data",
            "http:fetch",
            Some("success"),
            true,
        )
        .await?;

    state
        .record_action_pattern(
            "tool_usage",
            "fetch_data",
            "http:fetch",
            Some("success"),
            true,
        )
        .await?;

    state
        .record_action_pattern(
            "tool_usage",
            "fetch_data",
            "filesystem:read",
            Some("failure"),
            false,
        )
        .await?;

    // Check if we can retrieve the best action
    let best_action = state
        .get_best_action_for_context("tool_usage", "fetch_data", 0.5)
        .await?;

    assert!(best_action.is_some());
    let (action, confidence) = best_action.unwrap();
    assert_eq!(action, "http:fetch");
    assert!(confidence > 0.8);

    Ok(())
}

#[tokio::test]
async fn test_learning_metrics() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let db_path = temp_file.path().to_str().unwrap();

    let state = StateManager::new(db_path).await?;

    // Update some metrics
    state.update_learning_metric("success_rate", 0.75).await?;
    state.update_learning_metric("success_rate", 0.85).await?;
    state
        .update_learning_metric("tool_usage_count", 10.0)
        .await?;

    // Retrieve metrics
    let metrics = state.get_learning_metrics().await?;

    assert!(metrics.contains_key("success_rate"));
    assert!(metrics.contains_key("tool_usage_count"));

    // Check running average calculation
    let success_rate = metrics.get("success_rate").unwrap();
    assert!((success_rate - 0.80).abs() < 0.01); // Should be (0.75 + 0.85) / 2

    Ok(())
}

#[tokio::test]
async fn test_decision_pattern_analysis() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let db_path = temp_file.path().to_str().unwrap();

    let state = StateManager::new(db_path).await?;

    // Record some patterns
    state
        .record_action_pattern(
            "reasoning",
            "exploration",
            "explore",
            Some("discovered_tools"),
            true,
        )
        .await?;

    state
        .record_action_pattern(
            "reasoning",
            "tool_execution",
            "use_tool",
            Some("executed"),
            true,
        )
        .await?;

    // Analyze patterns
    let analysis = state.analyze_decision_patterns(24).await?;

    assert!(analysis.is_object());
    assert!(analysis["successful_patterns"].is_array());

    Ok(())
}

#[tokio::test]
async fn test_capability_tracking() -> Result<()> {
    let temp_file = NamedTempFile::new()?;
    let db_path = temp_file.path().to_str().unwrap();

    let state = StateManager::new(db_path).await?;

    // Record capability usage
    state
        .record_capability("http:fetch", Some("Fetches HTTP content"), true)
        .await?;

    state.record_capability("http:fetch", None, true).await?;

    state.record_capability("http:fetch", None, false).await?;

    // Get capabilities
    let capabilities = state.get_capabilities().await?;

    assert!(!capabilities.is_empty());

    // Find http:fetch capability
    let http_fetch = capabilities
        .iter()
        .find(|(name, _, _)| name == "http:fetch");

    assert!(http_fetch.is_some());
    let (_, desc, success_rate) = http_fetch.unwrap();
    assert!(desc.is_some());
    assert!(success_rate.is_some());

    // Success rate should be around 0.66 (2 successes, 1 failure with moving average)
    let rate = success_rate.unwrap();
    assert!(rate > 0.5 && rate < 1.0);

    Ok(())
}
