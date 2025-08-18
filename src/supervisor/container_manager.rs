use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tokio::process::Command;
use tokio::time::{Duration, timeout};
use tracing::{debug, error, info, warn};

use super::log_stream::LogStreamer;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerConfig {
    pub image: String,
    pub name: String,
    pub env_vars: HashMap<String, String>,
    pub volumes: Vec<String>,
    pub network: Option<String>,
    pub memory_limit: Option<String>,
    pub cpu_limit: Option<String>,
    pub restart_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStats {
    pub container_id: String,
    pub name: String,
    pub cpu_percent: f64,
    pub memory_usage_mb: f64,
    pub memory_limit_mb: f64,
    pub memory_percent: f64,
    pub network_rx_bytes: u64,
    pub network_tx_bytes: u64,
    pub block_read_bytes: u64,
    pub block_write_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub state: String,
    pub created: String,
    pub image: String,
    pub ports: Vec<String>,
}

pub struct ContainerManager {
    network_name: String,
}

impl ContainerManager {
    pub fn new(network_name: Option<String>) -> Self {
        Self {
            network_name: network_name.unwrap_or_else(|| "replicante-net".to_string()),
        }
    }

    pub async fn ensure_network(&self) -> Result<()> {
        info!("Ensuring Docker network '{}' exists", self.network_name);

        // Check if network exists
        let check_output = Command::new("docker")
            .arg("network")
            .arg("inspect")
            .arg(&self.network_name)
            .output()
            .await
            .context("Failed to check if network exists")?;

        if !check_output.status.success() {
            // Network doesn't exist, create it
            info!("Creating Docker network '{}'", self.network_name);

            let create_output = Command::new("docker")
                .arg("network")
                .arg("create")
                .arg("--driver")
                .arg("bridge")
                .arg(&self.network_name)
                .output()
                .await
                .context("Failed to create Docker network")?;

            if !create_output.status.success() {
                let stderr = String::from_utf8_lossy(&create_output.stderr);
                bail!("Failed to create network: {stderr}");
            }

            info!(
                "Successfully created Docker network '{}'",
                self.network_name
            );
        } else {
            debug!("Docker network '{}' already exists", self.network_name);
        }

        Ok(())
    }

    pub async fn create_container(&self, config: &ContainerConfig) -> Result<String> {
        info!(
            "Creating container '{}' from image '{}'",
            config.name, config.image
        );

        // Ensure network exists first
        if config.network.is_some() || !self.network_name.is_empty() {
            self.ensure_network().await?;
        }

        let mut cmd = Command::new("docker");
        cmd.arg("create");

        // Set container name
        cmd.arg("--name").arg(&config.name);

        // Set environment variables
        for (key, value) in &config.env_vars {
            cmd.arg("-e").arg(format!("{key}={value}"));
        }

        // Set volumes
        for volume in &config.volumes {
            cmd.arg("-v").arg(volume);
        }

        // Set network
        if let Some(network) = &config.network {
            cmd.arg("--network").arg(network);
        } else if !self.network_name.is_empty() {
            cmd.arg("--network").arg(&self.network_name);
        }

        // Set resource limits
        if let Some(memory) = &config.memory_limit {
            cmd.arg("--memory").arg(memory);
        }

        if let Some(cpu) = &config.cpu_limit {
            cmd.arg("--cpus").arg(cpu);
        }

        // Set restart policy
        if let Some(policy) = &config.restart_policy {
            cmd.arg("--restart").arg(policy);
        }

        // Add the image
        cmd.arg(&config.image);

        let output = cmd.output().await.context("Failed to create container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create container: {stderr}");
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();

        info!("Created container with ID: {container_id}");
        Ok(container_id)
    }

    pub async fn start_container(&self, container_id: &str) -> Result<()> {
        info!("Starting container {container_id}");

        let output = Command::new("docker")
            .arg("start")
            .arg(container_id)
            .output()
            .await
            .context("Failed to start container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to start container: {stderr}");
        }

        info!("Successfully started container {container_id}");
        Ok(())
    }

    pub async fn stop_container(&self, container_id: &str, timeout_secs: u64) -> Result<()> {
        info!("Stopping container {container_id} with timeout {timeout_secs}s");

        let output = Command::new("docker")
            .arg("stop")
            .arg("-t")
            .arg(timeout_secs.to_string())
            .arg(container_id)
            .output()
            .await
            .context("Failed to stop container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to stop container: {stderr}");
        }

        info!("Successfully stopped container {container_id}");
        Ok(())
    }

    pub async fn kill_container(&self, container_id: &str) -> Result<()> {
        warn!("Force killing container {container_id}");

        let output = Command::new("docker")
            .arg("kill")
            .arg(container_id)
            .output()
            .await
            .context("Failed to kill container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to kill container: {stderr}");
        }

        info!("Successfully killed container {container_id}");
        Ok(())
    }

    pub async fn remove_container(&self, container_id: &str, force: bool) -> Result<()> {
        info!("Removing container {container_id} (force: {force})");

        let mut cmd = Command::new("docker");
        cmd.arg("rm");

        if force {
            cmd.arg("-f");
        }

        cmd.arg(container_id);

        let output = cmd.output().await.context("Failed to remove container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to remove container: {stderr}");
        }

        info!("Successfully removed container {container_id}");
        Ok(())
    }

    pub async fn get_container_stats(&self, container_id: &str) -> Result<ContainerStats> {
        debug!("Getting stats for container {container_id}");

        // Use timeout to prevent hanging if container is unresponsive
        let stats_future = async {
            let output = Command::new("docker")
                .arg("stats")
                .arg("--no-stream")
                .arg("--format")
                .arg("{{json .}}")
                .arg(container_id)
                .output()
                .await
                .context("Failed to get container stats")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Failed to get container stats: {stderr}");
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let json_str = stdout.trim();

            if json_str.is_empty() {
                bail!("No stats returned for container {container_id}");
            }

            let json: Value =
                serde_json::from_str(json_str).context("Failed to parse stats JSON")?;

            // Parse the stats from Docker's format
            let container_id = json["Container"]
                .as_str()
                .unwrap_or(container_id)
                .to_string();

            let name = json["Name"].as_str().unwrap_or("unknown").to_string();

            // Parse CPU percentage (remove % sign)
            let cpu_str = json["CPUPerc"].as_str().unwrap_or("0%");
            let cpu_percent = cpu_str.trim_end_matches('%').parse::<f64>().unwrap_or(0.0);

            // Parse memory usage and limit
            let mem_str = json["MemUsage"].as_str().unwrap_or("0B / 0B");
            let (memory_usage_mb, memory_limit_mb) = parse_memory_usage(mem_str);

            // Parse memory percentage
            let mem_perc_str = json["MemPerc"].as_str().unwrap_or("0%");
            let memory_percent = mem_perc_str
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap_or(0.0);

            // Parse network I/O
            let net_str = json["NetIO"].as_str().unwrap_or("0B / 0B");
            let (network_rx_bytes, network_tx_bytes) = parse_io_stats(net_str);

            // Parse block I/O
            let block_str = json["BlockIO"].as_str().unwrap_or("0B / 0B");
            let (block_read_bytes, block_write_bytes) = parse_io_stats(block_str);

            Ok(ContainerStats {
                container_id,
                name,
                cpu_percent,
                memory_usage_mb,
                memory_limit_mb,
                memory_percent,
                network_rx_bytes,
                network_tx_bytes,
                block_read_bytes,
                block_write_bytes,
            })
        };

        // Apply timeout to prevent hanging
        match timeout(Duration::from_secs(10), stats_future).await {
            Ok(result) => result,
            Err(_) => {
                error!("Timeout getting stats for container {container_id}");
                bail!("Timeout getting container stats");
            }
        }
    }

    pub async fn get_container_info(&self, container_id: &str) -> Result<ContainerInfo> {
        debug!("Getting info for container {container_id}");

        let output = Command::new("docker")
            .arg("inspect")
            .arg("--format")
            .arg("{{json .}}")
            .arg(container_id)
            .output()
            .await
            .context("Failed to inspect container")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to inspect container: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: Value =
            serde_json::from_str(stdout.trim()).context("Failed to parse container info JSON")?;

        let id = json["Id"].as_str().unwrap_or("").chars().take(12).collect();

        let name = json["Name"]
            .as_str()
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string();

        let status = json["State"]["Status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let state = if json["State"]["Running"].as_bool().unwrap_or(false) {
            "running".to_string()
        } else if json["State"]["Paused"].as_bool().unwrap_or(false) {
            "paused".to_string()
        } else {
            "stopped".to_string()
        };

        let created = json["Created"].as_str().unwrap_or("").to_string();

        let image = json["Config"]["Image"].as_str().unwrap_or("").to_string();

        let ports = Vec::new(); // TODO: Parse ports if needed

        Ok(ContainerInfo {
            id,
            name,
            status,
            state,
            created,
            image,
            ports,
        })
    }

    pub async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        debug!("Listing containers (all: {all})");

        let mut cmd = Command::new("docker");
        cmd.arg("ps");

        if all {
            cmd.arg("-a");
        }

        cmd.arg("--format").arg("{{json .}}");

        let output = cmd.output().await.context("Failed to list containers")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to list containers: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut containers = Vec::new();

        for line in stdout.lines() {
            if line.trim().is_empty() {
                continue;
            }

            let json: Value =
                serde_json::from_str(line).context("Failed to parse container list JSON")?;

            let info = ContainerInfo {
                id: json["ID"].as_str().unwrap_or("").to_string(),
                name: json["Names"].as_str().unwrap_or("").to_string(),
                status: json["Status"].as_str().unwrap_or("").to_string(),
                state: json["State"].as_str().unwrap_or("").to_string(),
                created: json["CreatedAt"].as_str().unwrap_or("").to_string(),
                image: json["Image"].as_str().unwrap_or("").to_string(),
                ports: json["Ports"]
                    .as_str()
                    .unwrap_or("")
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            };

            containers.push(info);
        }

        Ok(containers)
    }

    pub fn get_log_streamer(&self, container_id: String) -> LogStreamer {
        LogStreamer::new(container_id)
    }
}

// Helper function to parse memory usage string like "100MiB / 1GiB"
fn parse_memory_usage(mem_str: &str) -> (f64, f64) {
    let parts: Vec<&str> = mem_str.split('/').collect();
    if parts.len() != 2 {
        return (0.0, 0.0);
    }

    let usage = parse_size_to_mb(parts[0].trim());
    let limit = parse_size_to_mb(parts[1].trim());

    (usage, limit)
}

// Helper function to parse I/O stats string like "100MB / 200MB"
fn parse_io_stats(io_str: &str) -> (u64, u64) {
    let parts: Vec<&str> = io_str.split('/').collect();
    if parts.len() != 2 {
        return (0, 0);
    }

    let rx = parse_size_to_bytes(parts[0].trim());
    let tx = parse_size_to_bytes(parts[1].trim());

    (rx, tx)
}

// Helper function to parse size string to MB
fn parse_size_to_mb(size_str: &str) -> f64 {
    if size_str.is_empty() || size_str == "--" {
        return 0.0;
    }

    // Remove any trailing characters and parse
    let size_str = size_str.to_uppercase();

    if let Some(pos) = size_str.find("GIB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val * 1024.0;
    }

    if let Some(pos) = size_str.find("GB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val * 1000.0;
    }

    if let Some(pos) = size_str.find("MIB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val;
    }

    if let Some(pos) = size_str.find("MB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val;
    }

    if let Some(pos) = size_str.find("KIB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val / 1024.0;
    }

    if let Some(pos) = size_str.find("KB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val / 1000.0;
    }

    if let Some(pos) = size_str.find('B')
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val / (1024.0 * 1024.0);
    }

    0.0
}

// Helper function to parse size string to bytes
fn parse_size_to_bytes(size_str: &str) -> u64 {
    if size_str.is_empty() || size_str == "--" {
        return 0;
    }

    let size_str = size_str.to_uppercase();

    if let Some(pos) = size_str.find("GB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return (val * 1_000_000_000.0) as u64;
    }

    if let Some(pos) = size_str.find("MB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return (val * 1_000_000.0) as u64;
    }

    if let Some(pos) = size_str.find("KB")
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return (val * 1_000.0) as u64;
    }

    if let Some(pos) = size_str.find('B')
        && let Ok(val) = size_str[..pos].trim().parse::<f64>()
    {
        return val as u64;
    }

    0
}
