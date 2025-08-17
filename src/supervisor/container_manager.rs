use anyhow::{Context, Result, bail};
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, LogsOptions, RemoveContainerOptions, Stats, StatsOptions,
    StopContainerOptions,
};
use bollard::image::CreateImageOptions;
use bollard::models::{
    ContainerStateStatusEnum, HostConfig, Mount, MountTypeEnum, RestartPolicy,
    RestartPolicyNameEnum,
};
use bollard::network::CreateNetworkOptions;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

use super::{SandboxConfig, SandboxMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub image: String,
    pub name: String,
    pub config_path: PathBuf,
    pub sandbox_config: Option<SandboxConfig>,
    pub environment: HashMap<String, String>,
    pub labels: HashMap<String, String>,
}

pub struct ContainerManager {
    docker: Docker,
    #[allow(dead_code)]
    default_image: String,
    network_name: String,
}

impl ContainerManager {
    pub async fn new() -> Result<Self> {
        // Connect to Docker
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to Docker. Is Docker running?")?;

        // Verify connection
        docker
            .ping()
            .await
            .context("Failed to ping Docker daemon")?;

        info!("Connected to Docker daemon");

        let manager = Self {
            docker,
            default_image: "replicante:latest".to_string(),
            network_name: "replicante-net".to_string(),
        };

        // Ensure the network exists
        manager.ensure_network().await?;

        Ok(manager)
    }

    async fn ensure_network(&self) -> Result<()> {
        // Check if network exists
        let networks = self.docker.list_networks::<String>(None).await?;
        let network_exists = networks
            .iter()
            .any(|n| n.name.as_deref() == Some(self.network_name.as_str()));

        if network_exists {
            debug!("Network {} already exists", self.network_name);
            Ok(())
        } else {
            info!("Creating Docker network: {}", self.network_name);

            let mut labels = HashMap::new();
            labels.insert("replicante.managed".to_string(), "true".to_string());

            let config = CreateNetworkOptions {
                name: self.network_name.clone(),
                check_duplicate: true,
                driver: "bridge".to_string(),
                labels,
                ..Default::default()
            };

            self.docker
                .create_network(config)
                .await
                .context("Failed to create Docker network")?;

            info!("Successfully created network: {}", self.network_name);
            Ok(())
        }
    }

    pub async fn create_agent_container(
        &self,
        agent_id: &str,
        config: ContainerConfig,
    ) -> Result<String> {
        info!("Creating container for agent {agent_id}");

        // Ensure image exists
        self.ensure_image(&config.image).await?;

        // Build container configuration
        let container_config = self.build_container_config(agent_id, &config)?;

        // Create container
        let options = CreateContainerOptions {
            name: config.name.clone(),
            platform: None,
        };

        let container = self
            .docker
            .create_container(Some(options), container_config)
            .await
            .context("Failed to create container")?;

        info!("Created container {} for agent {}", container.id, agent_id);

        Ok(container.id)
    }

    pub async fn start_container(&self, container_id: &str) -> Result<()> {
        info!("Starting container {container_id}");

        self.docker
            .start_container::<String>(container_id, None)
            .await
            .context("Failed to start container")?;

        Ok(())
    }

    pub async fn stop_container(&self, container_id: &str, timeout: Option<i64>) -> Result<()> {
        info!("Stopping container {container_id}");

        let options = StopContainerOptions {
            t: timeout.unwrap_or(30),
        };

        self.docker
            .stop_container(container_id, Some(options))
            .await
            .context("Failed to stop container")?;

        Ok(())
    }

    pub async fn kill_container(&self, container_id: &str) -> Result<()> {
        warn!("Force killing container {container_id}");

        self.docker
            .kill_container::<String>(container_id, None)
            .await
            .context("Failed to kill container")?;

        Ok(())
    }

    pub async fn pause_container(&self, container_id: &str) -> Result<()> {
        info!("Pausing container {container_id}");

        self.docker
            .pause_container(container_id)
            .await
            .context("Failed to pause container")?;

        Ok(())
    }

    pub async fn unpause_container(&self, container_id: &str) -> Result<()> {
        info!("Unpausing container {container_id}");

        self.docker
            .unpause_container(container_id)
            .await
            .context("Failed to unpause container")?;

        Ok(())
    }

    pub async fn remove_container(&self, container_id: &str, force: bool) -> Result<()> {
        info!("Removing container {container_id}");

        let options = RemoveContainerOptions {
            force,
            ..Default::default()
        };

        self.docker
            .remove_container(container_id, Some(options))
            .await
            .context("Failed to remove container")?;

        Ok(())
    }

    pub async fn restart_container(&self, container_id: &str) -> Result<()> {
        info!("Restarting container {container_id}");

        self.docker
            .restart_container(container_id, None)
            .await
            .context("Failed to restart container")?;

        Ok(())
    }

    pub async fn get_container_status(&self, container_id: &str) -> Result<ContainerStatus> {
        let container = self
            .docker
            .inspect_container(container_id, None)
            .await
            .context("Failed to inspect container")?;

        let state = container
            .state
            .ok_or_else(|| anyhow::anyhow!("Container has no state"))?;

        let status = match state.status {
            Some(ContainerStateStatusEnum::RUNNING) => ContainerStatus::Running,
            Some(ContainerStateStatusEnum::PAUSED) => ContainerStatus::Paused,
            Some(ContainerStateStatusEnum::EXITED) => ContainerStatus::Stopped,
            Some(ContainerStateStatusEnum::DEAD) => ContainerStatus::Dead,
            Some(ContainerStateStatusEnum::CREATED) => ContainerStatus::Created,
            Some(ContainerStateStatusEnum::RESTARTING) => ContainerStatus::Restarting,
            Some(ContainerStateStatusEnum::REMOVING) => ContainerStatus::Removing,
            _ => ContainerStatus::Unknown,
        };

        Ok(status)
    }

    pub async fn get_container_stats(&self, container_id: &str) -> Result<ContainerResourceUsage> {
        // First check if container is actually running
        match self.docker.inspect_container(container_id, None).await {
            Ok(info) => {
                if let Some(state) = info.state
                    && !state.running.unwrap_or(false)
                {
                    // Return zero stats for stopped container
                    return Ok(ContainerResourceUsage {
                        cpu_percent: 0.0,
                        memory_bytes: 0,
                        memory_percent: 0.0,
                        network_rx_bytes: 0,
                        network_tx_bytes: 0,
                        disk_read_bytes: 0,
                        disk_write_bytes: 0,
                    });
                }
            }
            Err(e) => {
                debug!(
                    "Container {} not found or not accessible: {}",
                    container_id, e
                );
                bail!("Container not found: {}", container_id);
            }
        }

        let options = StatsOptions {
            stream: false,
            one_shot: true,
        };

        let mut stream = self.docker.stats(container_id, Some(options));

        // Try to get stats with timeout
        let stats_result =
            tokio::time::timeout(std::time::Duration::from_secs(2), stream.next()).await;

        match stats_result {
            Ok(Some(Ok(stats))) => {
                let cpu_percent = calculate_cpu_percentage(&stats);
                let memory_usage = stats.memory_stats.usage.unwrap_or(0);
                let memory_limit = stats.memory_stats.limit.unwrap_or(0);
                let memory_percent = if memory_limit > 0 {
                    (memory_usage as f64 / memory_limit as f64) * 100.0
                } else {
                    0.0
                };

                Ok(ContainerResourceUsage {
                    cpu_percent,
                    memory_bytes: memory_usage,
                    memory_percent,
                    network_rx_bytes: stats
                        .networks
                        .as_ref()
                        .and_then(|n| n.get("eth0"))
                        .map(|n| n.rx_bytes)
                        .unwrap_or(0),
                    network_tx_bytes: stats
                        .networks
                        .as_ref()
                        .and_then(|n| n.get("eth0"))
                        .map(|n| n.tx_bytes)
                        .unwrap_or(0),
                    disk_read_bytes: stats
                        .blkio_stats
                        .io_service_bytes_recursive
                        .as_ref()
                        .map(|ios| {
                            ios.iter()
                                .filter(|io| io.op == "read")
                                .map(|io| io.value)
                                .sum()
                        })
                        .unwrap_or(0),
                    disk_write_bytes: stats
                        .blkio_stats
                        .io_service_bytes_recursive
                        .as_ref()
                        .map(|ios| {
                            ios.iter()
                                .filter(|io| io.op == "write")
                                .map(|io| io.value)
                                .sum()
                        })
                        .unwrap_or(0),
                })
            }
            Ok(Some(Err(e))) => {
                debug!("Stats API error for container {}: {}", container_id, e);
                bail!("Failed to get container stats: {}", e)
            }
            Ok(None) => {
                debug!("No stats available for container {}", container_id);
                bail!("No stats available")
            }
            Err(_) => {
                debug!("Timeout getting stats for container {}", container_id);
                // Return zero stats on timeout instead of failing
                Ok(ContainerResourceUsage {
                    cpu_percent: 0.0,
                    memory_bytes: 0,
                    memory_percent: 0.0,
                    network_rx_bytes: 0,
                    network_tx_bytes: 0,
                    disk_read_bytes: 0,
                    disk_write_bytes: 0,
                })
            }
        }
    }

    pub async fn stream_container_logs(
        &self,
        container_id: &str,
        follow: bool,
        tail: Option<String>,
    ) -> Result<impl futures::Stream<Item = Result<String>>> {
        let options = LogsOptions {
            follow,
            stdout: true,
            stderr: true,
            tail: tail.unwrap_or_else(|| "all".to_string()),
            timestamps: true,
            ..Default::default()
        };

        let stream = self.docker.logs(container_id, Some(options));

        Ok(stream.map(|result| {
            result
                .map(|log| log.to_string())
                .map_err(|e| anyhow::anyhow!("Log streaming error: {}", e))
        }))
    }

    async fn ensure_image(&self, image: &str) -> Result<()> {
        // Check if image exists locally
        match self.docker.inspect_image(image).await {
            Ok(_) => {
                debug!("Image {image} already exists locally");
                Ok(())
            }
            Err(_) => {
                info!("Pulling image {image}");

                let options = CreateImageOptions {
                    from_image: image,
                    ..Default::default()
                };

                let mut stream = self.docker.create_image(Some(options), None, None);

                while let Some(result) = stream.next().await {
                    match result {
                        Ok(info) => {
                            if let Some(status) = info.status {
                                debug!("Pull status: {status}");
                            }
                        }
                        Err(e) => {
                            error!("Failed to pull image: {e}");
                            bail!("Failed to pull image {image}: {e}");
                        }
                    }
                }

                info!("Successfully pulled image {image}");
                Ok(())
            }
        }
    }

    fn build_container_config(
        &self,
        agent_id: &str,
        config: &ContainerConfig,
    ) -> Result<Config<String>> {
        let mut env = vec![format!("AGENT_ID={agent_id}"), format!("RUST_LOG=info")];

        // Add custom environment variables
        for (key, value) in &config.environment {
            env.push(format!("{key}={value}"));
        }

        // Build labels
        let mut labels = HashMap::new();
        labels.insert("replicante.agent.id".to_string(), agent_id.to_string());
        labels.insert("replicante.managed".to_string(), "true".to_string());
        for (key, value) in &config.labels {
            labels.insert(key.clone(), value.clone());
        }

        // Build mounts
        let mounts = vec![
            // Config mount (read-only)
            Mount {
                target: Some("/config/agent.toml".to_string()),
                source: Some(config.config_path.to_string_lossy().to_string()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(true),
                ..Default::default()
            },
            // Data volume
            Mount {
                target: Some("/data".to_string()),
                source: Some(format!("agent-data-{agent_id}")),
                typ: Some(MountTypeEnum::VOLUME),
                ..Default::default()
            },
            // Workspace volume
            Mount {
                target: Some("/workspace".to_string()),
                source: Some(format!("agent-workspace-{agent_id}")),
                typ: Some(MountTypeEnum::VOLUME),
                ..Default::default()
            },
        ];

        // Build host config based on sandbox settings
        let mut host_config = HostConfig {
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            network_mode: Some(self.network_name.clone()),
            mounts: Some(mounts),
            ..Default::default()
        };

        // Apply sandbox configuration if provided
        if let Some(sandbox) = &config.sandbox_config {
            host_config = self.apply_sandbox_config(host_config, sandbox)?;
        }

        Ok(Config {
            image: Some(config.image.clone()),
            hostname: Some(format!("agent-{agent_id}")),
            env: Some(env),
            labels: Some(labels),
            host_config: Some(host_config),
            working_dir: Some("/home/replicante".to_string()),
            user: Some("replicante:replicante".to_string()),
            cmd: Some(vec![
                "agent".to_string(),
                "--config".to_string(),
                "/config/agent.toml".to_string(),
            ]),
            ..Default::default()
        })
    }

    fn apply_sandbox_config(
        &self,
        mut host_config: HostConfig,
        sandbox: &SandboxConfig,
    ) -> Result<HostConfig> {
        // Apply resource limits
        {
            let limits = &sandbox.resources;
            // CPU limits (in nanocpus, 1 CPU = 1e9 nanocpus)
            host_config.nano_cpus = Some((limits.max_cpu_percent * 1e7) as i64);

            // Memory limits (in bytes)
            host_config.memory = Some((limits.max_memory_mb * 1024 * 1024) as i64);

            // PIDs limit
            host_config.pids_limit = Some(limits.max_processes as i64);
        }

        // Apply security options
        match sandbox.mode {
            SandboxMode::Strict => {
                host_config.readonly_rootfs = Some(true);
                host_config.network_mode = Some("none".to_string());
                host_config.cap_drop = Some(vec!["ALL".to_string()]);
                host_config.cap_add = Some(vec![]);
                host_config.security_opt = Some(vec![
                    "no-new-privileges:true".to_string(),
                    "apparmor:docker-default".to_string(),
                ]);
            }
            SandboxMode::Moderate => {
                host_config.readonly_rootfs = Some(false);
                host_config.cap_drop = Some(vec!["ALL".to_string()]);
                host_config.cap_add = Some(vec!["NET_BIND_SERVICE".to_string()]);
                host_config.security_opt = Some(vec![
                    "no-new-privileges:true".to_string(),
                    "apparmor:docker-default".to_string(),
                ]);
            }
            SandboxMode::Permissive => {
                host_config.readonly_rootfs = Some(false);
                host_config.cap_drop =
                    Some(vec!["SYS_ADMIN".to_string(), "SYS_MODULE".to_string()]);
                host_config.security_opt = Some(vec!["apparmor:docker-default".to_string()]);
            }
        }

        // Add tmpfs for temporary files if root is read-only
        if host_config.readonly_rootfs == Some(true) {
            host_config.tmpfs = Some(HashMap::from([
                ("/tmp".to_string(), "size=100m".to_string()),
                ("/run".to_string(), "size=10m".to_string()),
            ]));
        }

        Ok(host_config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContainerStatus {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Stopped,
    Dead,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerResourceUsage {
    pub cpu_percent: f64,
    pub memory_bytes: u64,
    pub memory_percent: f64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub disk_read_bytes: u64,
    pub disk_write_bytes: u64,
}

fn calculate_cpu_percentage(stats: &Stats) -> f64 {
    let cpu_stats = &stats.cpu_stats;
    let precpu_stats = &stats.precpu_stats;

    if let (Some(system_cpu_usage), Some(system_precpu_usage)) =
        (cpu_stats.system_cpu_usage, precpu_stats.system_cpu_usage)
    {
        let cpu_usage = cpu_stats.cpu_usage.total_usage;
        let precpu_usage = precpu_stats.cpu_usage.total_usage;

        let cpu_delta = cpu_usage as f64 - precpu_usage as f64;
        let system_delta = system_cpu_usage as f64 - system_precpu_usage as f64;

        if system_delta > 0.0 && cpu_delta > 0.0 {
            let num_cpus = cpu_stats
                .online_cpus
                .or_else(|| {
                    cpu_stats
                        .cpu_usage
                        .percpu_usage
                        .as_ref()
                        .map(|p| p.len() as u64)
                })
                .unwrap_or(1) as f64;

            return (cpu_delta / system_delta) * num_cpus * 100.0;
        }
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires Docker to be running
    async fn test_container_manager_connection() -> Result<()> {
        let manager = ContainerManager::new().await?;
        assert!(!manager.default_image.is_empty());
        Ok(())
    }
}
