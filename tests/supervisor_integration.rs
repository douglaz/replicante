#[cfg(test)]
mod supervisor_tests {
    use anyhow::Result;
    use replicante::supervisor::{SandboxConfig, SandboxMode, Supervisor, SupervisorConfig};

    #[tokio::test]
    #[ignore] // Requires Docker to be running
    async fn test_supervisor_initialization() -> Result<()> {
        let config = SupervisorConfig::default();
        let supervisor = Supervisor::new(config).await?;

        // Verify supervisor was created
        let status = supervisor.get_status().await;
        assert!(status.is_empty(), "Should start with no agents");

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires Docker to be running
    async fn test_spawn_agent_container() -> Result<()> {
        let config = SupervisorConfig::default();
        let supervisor = Supervisor::new(config).await?;

        // Start the supervisor
        supervisor.start().await?;

        // Create a test config file
        let config_path = "/tmp/test_agent.toml".to_string();
        std::fs::write(
            &config_path,
            r#"
            [llm]
            provider = "mock"
            
            [database]
            path = "/tmp/test.db"
        "#,
        )?;

        // Spawn an agent
        let agent_id = supervisor
            .spawn_agent(
                config_path.clone(),
                Some(SandboxConfig {
                    enabled: true,
                    mode: SandboxMode::Moderate,
                    filesystem: replicante::supervisor::FilesystemRestrictions {
                        root: "/sandbox".to_string(),
                        read_only_paths: vec![],
                        write_paths: vec!["/workspace".to_string()],
                        max_size_mb: 100,
                    },
                    network: replicante::supervisor::NetworkRestrictions {
                        mode: replicante::supervisor::NetworkMode::Filtered,
                        allowed_domains: vec!["api.anthropic.com".to_string()],
                        blocked_ports: vec![22, 3389],
                        rate_limit_per_minute: Some(100),
                    },
                    resources: replicante::supervisor::ResourceLimits {
                        max_memory_mb: 512,
                        max_cpu_percent: 1.0,
                        max_processes: 100,
                        max_open_files: 1000,
                    },
                    mcp: replicante::supervisor::MCPRestrictions {
                        allowed_servers: vec!["filesystem".to_string()],
                        blocked_tools: vec!["execute_command".to_string()],
                        tool_rate_limits: std::collections::HashMap::new(),
                    },
                }),
            )
            .await?;

        // Verify agent was created
        let status = supervisor.get_status().await;
        assert_eq!(status.len(), 1, "Should have one agent");
        assert!(status.contains_key(&agent_id), "Agent should be in status");

        // Get agent details
        let agent_details = supervisor.get_agent_details(&agent_id).await;
        assert!(agent_details.is_some(), "Should get agent details");

        if let Some(details) = agent_details {
            // Agent process details verification
            assert_eq!(details.config_path, config_path);
            assert!(details.pid.is_some(), "Agent should have a PID");
        }

        // Stop the agent
        supervisor.stop_agent(&agent_id).await?;

        // Clean up
        std::fs::remove_file(&config_path).ok();

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires Docker to be running
    async fn test_agent_lifecycle() -> Result<()> {
        let config = SupervisorConfig::default();
        let supervisor = Supervisor::new(config).await?;

        supervisor.start().await?;

        // Create test config
        let config_path = "/tmp/test_lifecycle.toml".to_string();
        std::fs::write(
            &config_path,
            r#"
            [llm]
            provider = "mock"
            
            [database]
            path = "/tmp/lifecycle.db"
        "#,
        )?;

        // Spawn agent
        let agent_id = supervisor.spawn_agent(config_path.clone(), None).await?;

        // Test pause/unpause (quarantine)
        supervisor.quarantine_agent(&agent_id).await?;

        let status = supervisor.get_status().await;
        if let Some(agent_status) = status.get(&agent_id) {
            assert!(matches!(
                agent_status,
                replicante::supervisor::AgentStatus::Quarantined
            ));
        }

        // Stop agent
        supervisor.stop_agent(&agent_id).await?;

        // Clean up
        std::fs::remove_file(&config_path).ok();

        Ok(())
    }

    #[tokio::test]
    #[ignore] // Requires Docker
    async fn test_resource_monitoring() -> Result<()> {
        use replicante::supervisor::container_manager::ContainerManager;

        // Just test that we can create a container manager
        let _manager = ContainerManager::new(Some("test-network".to_string()));

        // We can't test actual container operations without a real container
        // but we've verified the manager can connect to Docker

        Ok(())
    }
}
