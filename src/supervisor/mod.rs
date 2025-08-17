use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, interval};
use tracing::{error, info, warn};
use uuid::Uuid;

pub mod api;
pub mod async_client;
pub mod client;
pub mod container_manager;
pub mod daemon;
pub mod log_stream;
pub mod monitor;
pub mod security;

use container_manager::{ContainerConfig, ContainerManager};
use monitor::{Alert, Monitor};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorConfig {
    pub max_agents: usize,
    pub monitor_interval_secs: u64,
    pub web_port: Option<u16>,
    pub enable_dashboard: bool,
    pub log_level: String,
    pub alerts: AlertConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub max_cpu_percent: f64,
    pub max_memory_mb: u64,
    pub max_tool_calls_per_minute: u32,
    pub suspicious_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProcess {
    pub id: String,
    pub container_id: Option<String>,
    pub container_name: String,
    pub image: String,
    pub config_path: String,
    pub sandbox_config: Option<SandboxConfig>,
    pub status: AgentStatus,
    pub started_at: DateTime<Utc>,
    pub resource_usage: ResourceUsage,
    pub tool_usage: HashMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStatus {
    Starting,
    Running,
    Stopped,
    Crashed,
    Quarantined,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceUsage {
    pub cpu_percent: f64,
    pub memory_mb: u64,
    pub disk_io_bytes: u64,
    pub network_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: SandboxMode,
    pub filesystem: FilesystemRestrictions,
    pub network: NetworkRestrictions,
    pub resources: ResourceLimits,
    pub mcp: MCPRestrictions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SandboxMode {
    Strict,
    Moderate,
    Permissive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemRestrictions {
    pub root: String,
    pub read_only_paths: Vec<String>,
    pub write_paths: Vec<String>,
    pub max_size_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRestrictions {
    pub mode: NetworkMode,
    pub allowed_domains: Vec<String>,
    pub blocked_ports: Vec<u16>,
    pub rate_limit_per_minute: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMode {
    None,
    Filtered,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub max_memory_mb: u64,
    pub max_cpu_percent: f64,
    pub max_processes: u32,
    pub max_open_files: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPRestrictions {
    pub allowed_servers: Vec<String>,
    pub blocked_tools: Vec<String>,
    pub tool_rate_limits: HashMap<String, u32>,
}

pub struct Supervisor {
    config: SupervisorConfig,
    agents: Arc<RwLock<HashMap<String, AgentProcess>>>,
    monitor: Arc<Monitor>,
    container_manager: Arc<ContainerManager>,
    running: Arc<Mutex<bool>>,
}

impl Supervisor {
    pub async fn new(config: SupervisorConfig) -> Result<Self> {
        let monitor = Arc::new(Monitor::new());
        let container_manager = Arc::new(ContainerManager::new().await?);

        Ok(Self {
            config,
            agents: Arc::new(RwLock::new(HashMap::new())),
            monitor,
            container_manager,
            running: Arc::new(Mutex::new(false)),
        })
    }

    pub async fn start(self: Arc<Self>) -> Result<()> {
        let mut running = self.running.lock().await;
        if *running {
            bail!("Supervisor is already running");
        }
        *running = true;

        info!("Starting supervisor daemon");

        // Start monitoring loop
        let monitor = self.monitor.clone();
        let agents = self.agents.clone();
        let config = self.config.clone();
        let container_manager = self.container_manager.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(config.monitor_interval_secs));

            loop {
                interval.tick().await;

                // Check agent health and update resource usage
                let agents_guard = agents.read().await;
                for (id, agent) in agents_guard.iter() {
                    if let Err(e) = monitor.check_agent_health(agent).await {
                        error!("Health check failed for agent {id}: {e}");
                    }
                }
                drop(agents_guard);

                // Update resource usage from container stats
                if let Err(e) =
                    Self::update_resource_usage(agents.clone(), container_manager.clone()).await
                {
                    error!("Failed to update resource usage: {e}");
                }
            }
        });

        // Start web dashboard if enabled
        if self.config.enable_dashboard
            && let Some(port) = self.config.web_port
        {
            self.start_dashboard(port, self.clone()).await?;
        }

        Ok(())
    }

    pub async fn spawn_agent(
        &self,
        config_path: String,
        sandbox_config: Option<SandboxConfig>,
    ) -> Result<String> {
        let agent_id = Uuid::new_v4().to_string();
        let container_name = format!("replicante-agent-{}", agent_id);

        info!("Spawning agent {agent_id} with config: {config_path}");

        // Check max agents limit
        let agents_count = self.agents.read().await.len();
        if agents_count >= self.config.max_agents {
            bail!(
                "Maximum number of agents ({}) reached",
                self.config.max_agents
            );
        }

        // Prepare container configuration
        let mut environment = HashMap::new();
        environment.insert("AGENT_ID".to_string(), agent_id.clone());
        environment.insert("RUST_LOG".to_string(), self.config.log_level.clone());

        if let Some(ref sandbox) = sandbox_config
            && sandbox.enabled
        {
            environment.insert("SANDBOX_MODE".to_string(), format!("{:?}", sandbox.mode));
            environment.insert("SANDBOX_ROOT".to_string(), sandbox.filesystem.root.clone());
        }

        let mut labels = HashMap::new();
        labels.insert("replicante.supervisor".to_string(), "true".to_string());
        labels.insert("replicante.agent.id".to_string(), agent_id.clone());

        let container_config = ContainerConfig {
            image: "replicante:latest".to_string(),
            name: container_name.clone(),
            config_path: std::path::PathBuf::from(&config_path),
            sandbox_config: sandbox_config.clone(),
            environment,
            labels,
        };

        // Create and start container
        let container_id = self
            .container_manager
            .create_agent_container(&agent_id, container_config)
            .await
            .context("Failed to create agent container")?;

        self.container_manager
            .start_container(&container_id)
            .await
            .context("Failed to start agent container")?;

        // Create agent process entry
        let agent_process = AgentProcess {
            id: agent_id.clone(),
            container_id: Some(container_id.clone()),
            container_name,
            image: "replicante:latest".to_string(),
            config_path,
            sandbox_config,
            status: AgentStatus::Starting,
            started_at: Utc::now(),
            resource_usage: ResourceUsage::default(),
            tool_usage: HashMap::new(),
        };

        // Store agent
        self.agents
            .write()
            .await
            .insert(agent_id.clone(), agent_process);

        // Start monitoring this agent
        self.monitor.start_monitoring(&agent_id).await?;

        // Update status to running after successful start
        if let Some(agent) = self.agents.write().await.get_mut(&agent_id) {
            agent.status = AgentStatus::Running;
        }

        Ok(agent_id)
    }

    pub async fn stop_agent(&self, agent_id: &str) -> Result<()> {
        info!("Stopping agent {agent_id}");

        let mut agents = self.agents.write().await;

        if let Some(agent) = agents.get_mut(agent_id) {
            agent.status = AgentStatus::Stopped;

            // Stop container if it has a container ID
            if let Some(container_id) = &agent.container_id {
                self.container_manager
                    .stop_container(container_id, Some(30))
                    .await
                    .context("Failed to stop container")?;
            }

            // Stop monitoring
            self.monitor.stop_monitoring(agent_id).await?;

            Ok(())
        } else {
            bail!("Agent {agent_id} not found")
        }
    }

    pub async fn emergency_stop(&self, agent_id: &str) -> Result<()> {
        warn!("Emergency stop requested for agent {agent_id}");

        let mut agents = self.agents.write().await;

        if let Some(agent) = agents.get_mut(agent_id) {
            agent.status = AgentStatus::Stopped;

            // Kill container for immediate termination
            if let Some(container_id) = &agent.container_id {
                self.container_manager
                    .kill_container(container_id)
                    .await
                    .context("Failed to kill container")?;
            }

            // Generate incident report
            self.monitor.generate_incident_report(agent_id).await?;

            Ok(())
        } else {
            bail!("Agent {agent_id} not found")
        }
    }

    pub async fn quarantine_agent(&self, agent_id: &str) -> Result<()> {
        warn!("Quarantining agent {agent_id}");

        let mut agents = self.agents.write().await;

        if let Some(agent) = agents.get_mut(agent_id) {
            agent.status = AgentStatus::Quarantined;

            // Pause container to freeze it
            if let Some(container_id) = &agent.container_id {
                self.container_manager
                    .pause_container(container_id)
                    .await
                    .context("Failed to pause container")?;
            }

            // Alert monitoring system
            self.monitor
                .alert(Alert::AgentQuarantined {
                    agent_id: agent_id.to_string(),
                    reason: "Manual quarantine".to_string(),
                })
                .await?;

            Ok(())
        } else {
            bail!("Agent {agent_id} not found")
        }
    }

    pub async fn get_status(&self) -> HashMap<String, AgentStatus> {
        let agents = self.agents.read().await;
        agents
            .iter()
            .map(|(id, agent)| (id.clone(), agent.status.clone()))
            .collect()
    }

    pub async fn get_agent_details(&self, agent_id: &str) -> Option<AgentProcess> {
        let agents = self.agents.read().await;
        agents.get(agent_id).cloned()
    }

    pub fn get_agents_ref(&self) -> Arc<RwLock<HashMap<String, AgentProcess>>> {
        self.agents.clone()
    }

    pub fn get_monitor_ref(&self) -> Arc<Monitor> {
        self.monitor.clone()
    }

    pub fn get_container_manager_ref(&self) -> Arc<ContainerManager> {
        self.container_manager.clone()
    }

    pub async fn start_dashboard(&self, port: u16, supervisor: Arc<Supervisor>) -> Result<()> {
        info!("Starting web dashboard on port {port}");

        api::start_dashboard_server(port, supervisor).await?;

        Ok(())
    }

    async fn update_resource_usage(
        agents: Arc<RwLock<HashMap<String, AgentProcess>>>,
        container_manager: Arc<ContainerManager>,
    ) -> Result<()> {
        let agents_read = agents.read().await;
        let agent_ids: Vec<(String, Option<String>)> = agents_read
            .iter()
            .map(|(id, agent)| (id.clone(), agent.container_id.clone()))
            .collect();
        drop(agents_read);

        for (agent_id, container_id) in agent_ids {
            if let Some(container_id) = container_id {
                match container_manager.get_container_stats(&container_id).await {
                    Ok(stats) => {
                        let mut agents_write = agents.write().await;
                        if let Some(agent) = agents_write.get_mut(&agent_id) {
                            agent.resource_usage = ResourceUsage {
                                cpu_percent: stats.cpu_percent,
                                memory_mb: stats.memory_bytes / (1024 * 1024),
                                disk_io_bytes: stats.disk_read_bytes + stats.disk_write_bytes,
                                network_bytes: stats.network_rx_bytes + stats.network_tx_bytes,
                            };
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get stats for container {container_id}: {e}");
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            max_agents: 10,
            monitor_interval_secs: 5,
            web_port: Some(8080),
            enable_dashboard: true,
            log_level: "info".to_string(),
            alerts: AlertConfig {
                max_cpu_percent: 80.0,
                max_memory_mb: 512,
                max_tool_calls_per_minute: 100,
                suspicious_patterns: vec![
                    "rm -rf".to_string(),
                    "curl | sh".to_string(),
                    "/etc/passwd".to_string(),
                ],
            },
        }
    }
}
