use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{Duration, interval};
use tracing::{error, info, warn};
use uuid::Uuid;

pub mod api;
pub mod async_client;
pub mod container_manager;
pub mod daemon;
pub mod log_stream;
pub mod monitor;
pub mod security;

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
    pub pid: Option<u32>,
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
    running: Arc<Mutex<bool>>,
}

impl Supervisor {
    pub async fn new(config: SupervisorConfig) -> Result<Self> {
        let monitor = Arc::new(Monitor::new());

        Ok(Self {
            config,
            agents: Arc::new(RwLock::new(HashMap::new())),
            monitor,
            running: Arc::new(Mutex::new(false)),
        })
    }

    pub async fn start(&self) -> Result<()> {
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

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(config.monitor_interval_secs));

            loop {
                interval.tick().await;

                // Check agent health
                let agents_guard = agents.read().await;
                for (id, agent) in agents_guard.iter() {
                    if let Err(e) = monitor.check_agent_health(agent).await {
                        error!("Health check failed for agent {id}: {e}");
                    }
                }
            }
        });

        // Start web dashboard if enabled
        if self.config.enable_dashboard
            && let Some(port) = self.config.web_port
        {
            self.start_dashboard(port).await?;
        }

        Ok(())
    }

    pub async fn spawn_agent(
        &self,
        config_path: String,
        sandbox_config: Option<SandboxConfig>,
    ) -> Result<String> {
        let agent_id = format!("agent-{}", Uuid::new_v4());

        info!("Spawning agent {agent_id} with config: {config_path}");

        // Check max agents limit
        let agents_count = self.agents.read().await.len();
        if agents_count >= self.config.max_agents {
            bail!(
                "Maximum number of agents ({}) reached",
                self.config.max_agents
            );
        }

        // Build command
        let mut cmd = Command::new("replicante");

        if sandbox_config.is_some() {
            cmd.arg("sandbox");
        } else {
            cmd.arg("agent");
        }

        cmd.arg("--config").arg(&config_path);

        if let Some(ref sandbox) = sandbox_config {
            // Add sandbox arguments
            if sandbox.enabled {
                cmd.env("SANDBOX_MODE", format!("{:?}", sandbox.mode));
                cmd.env("SANDBOX_ROOT", &sandbox.filesystem.root);
            }
        }

        // Spawn the process
        let child = cmd.spawn().context("Failed to spawn agent process")?;

        let pid = child.id();

        // Create agent process entry
        let agent_process = AgentProcess {
            id: agent_id.clone(),
            pid,
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

        Ok(agent_id)
    }

    pub async fn stop_agent(&self, agent_id: &str) -> Result<()> {
        info!("Stopping agent {agent_id}");

        let mut agents = self.agents.write().await;

        if let Some(agent) = agents.get_mut(agent_id) {
            agent.status = AgentStatus::Stopped;

            // Send SIGTERM to process if it has a PID
            if let Some(pid) = agent.pid {
                std::process::Command::new("kill")
                    .arg("-TERM")
                    .arg(pid.to_string())
                    .output()?;
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

            // Send SIGKILL for immediate termination
            if let Some(pid) = agent.pid {
                std::process::Command::new("kill")
                    .arg("-KILL")
                    .arg(pid.to_string())
                    .output()?;
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

            // Send SIGSTOP to freeze the process
            if let Some(pid) = agent.pid {
                std::process::Command::new("kill")
                    .arg("-STOP")
                    .arg(pid.to_string())
                    .output()?;
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

    async fn start_dashboard(&self, port: u16) -> Result<()> {
        info!("Starting web dashboard on port {port}");

        // Dashboard implementation will be in api.rs
        api::start_dashboard_server(port, self.agents.clone(), self.monitor.clone()).await?;

        Ok(())
    }
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self {
            max_agents: 10,
            monitor_interval_secs: 5,
            web_port: Some(8090),
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
