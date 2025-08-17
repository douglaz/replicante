use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tracing::{error, info};

use super::{Supervisor, SupervisorConfig};

pub struct Daemon {
    supervisor: Arc<Supervisor>,
    _config_path: Option<PathBuf>,
}

impl Daemon {
    pub async fn new(config_path: Option<PathBuf>) -> Result<Self> {
        let config = if let Some(path) = &config_path {
            let contents = tokio::fs::read_to_string(path)
                .await
                .context("Failed to read supervisor config")?;
            toml::from_str(&contents)?
        } else {
            SupervisorConfig::default()
        };

        Self::new_with_config(config).await
    }

    pub async fn new_with_config(config: SupervisorConfig) -> Result<Self> {
        let supervisor = Arc::new(Supervisor::new(config).await?);

        Ok(Self {
            supervisor,
            _config_path: None,
        })
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting supervisor daemon");

        // Start the supervisor
        self.supervisor.clone().start().await?;

        // Wait for shutdown signal
        let shutdown = Self::shutdown_signal();

        tokio::select! {
            _ = shutdown => {
                info!("Received shutdown signal");
            }
        }

        // Graceful shutdown
        self.shutdown().await?;

        Ok(())
    }

    async fn shutdown(&self) -> Result<()> {
        info!("Shutting down supervisor daemon");

        // Stop all agents gracefully
        let status = self.supervisor.get_status().await;
        for (agent_id, _) in status {
            if let Err(e) = self.supervisor.stop_agent(&agent_id).await {
                error!("Failed to stop agent {agent_id}: {e}");
            }
        }

        info!("Supervisor daemon stopped");
        Ok(())
    }

    async fn shutdown_signal() {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("Failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }
    }
}
