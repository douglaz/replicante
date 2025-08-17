use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info};

#[derive(Debug, Clone)]
pub struct AsyncSupervisorClient {
    client: Client,
    base_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub agents: Vec<AgentInfo>,
    pub total_agents: usize,
    pub running_agents: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub status: String,
    pub config_path: String,
    pub started_at: String,
    pub cpu_percent: f64,
    pub memory_mb: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetricsResponse {
    pub metrics: Vec<AgentMetrics>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub agent_id: String,
    pub cpu_percent: f64,
    pub memory_mb: u64,
    pub disk_mb: u64,
    pub network_bytes_sent: u64,
    pub network_bytes_recv: u64,
}

impl AsyncSupervisorClient {
    pub fn new(base_url: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self { client, base_url })
    }

    pub async fn get_status(&self) -> Result<StatusResponse> {
        let url = format!("{}/api/status", self.base_url);
        debug!("Fetching supervisor status from {url}");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send status request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Status request failed with {status}: {text}");
            anyhow::bail!("Status request failed with {status}");
        }

        let status = response
            .json::<StatusResponse>()
            .await
            .context("Failed to parse status response")?;

        info!(
            "Retrieved status: {} agents ({} running)",
            status.total_agents, status.running_agents
        );

        Ok(status)
    }

    pub async fn get_metrics(&self) -> Result<MetricsResponse> {
        let url = format!("{}/api/metrics", self.base_url);
        debug!("Fetching metrics from {url}");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to send metrics request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Metrics request failed with {status}: {text}");
            anyhow::bail!("Metrics request failed with {status}");
        }

        let metrics = response
            .json::<MetricsResponse>()
            .await
            .context("Failed to parse metrics response")?;

        debug!("Retrieved metrics for {} agents", metrics.metrics.len());

        Ok(metrics)
    }

    pub async fn stop_agent(&self, agent_id: &str) -> Result<()> {
        let url = format!("{}/api/agents/{}/stop", self.base_url, agent_id);
        info!("Stopping agent {agent_id}");

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .context("Failed to send stop request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Stop request failed with {status}: {text}");
            anyhow::bail!("Stop request failed with {status}");
        }

        info!("Successfully stopped agent {agent_id}");
        Ok(())
    }

    pub async fn quarantine_agent(&self, agent_id: &str) -> Result<()> {
        let url = format!("{}/api/agents/{}/quarantine", self.base_url, agent_id);
        info!("Quarantining agent {agent_id}");

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .context("Failed to send quarantine request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Quarantine request failed with {status}: {text}");
            anyhow::bail!("Quarantine request failed with {status}");
        }

        info!("Successfully quarantined agent {agent_id}");
        Ok(())
    }

    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);

        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(e) => {
                debug!("Health check failed: {e}");
                Ok(false)
            }
        }
    }
}
