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
    pub fn new(base_url: Option<String>) -> Result<Self> {
        let base_url = base_url.unwrap_or_else(|| {
            std::env::var("SUPERVISOR_URL").unwrap_or_else(|_| "http://localhost:8090".to_string())
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self { client, base_url })
    }

    pub async fn get_status(&self) -> Result<StatusResponse> {
        let url = format!("{base_url}/api/status", base_url = self.base_url);
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
            "Retrieved status: {total} agents ({running} running)",
            total = status.total_agents,
            running = status.running_agents
        );

        Ok(status)
    }

    pub async fn get_metrics(&self) -> Result<MetricsResponse> {
        let url = format!("{base_url}/api/metrics", base_url = self.base_url);
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

        debug!(
            "Retrieved metrics for {count} agents",
            count = metrics.metrics.len()
        );

        Ok(metrics)
    }

    pub async fn stop_agent(&self, agent_id: &str) -> Result<()> {
        let url = format!(
            "{base_url}/api/agents/{agent_id}/stop",
            base_url = self.base_url
        );
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
        let url = format!(
            "{base_url}/api/agents/{agent_id}/quarantine",
            base_url = self.base_url
        );
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
        let url = format!("{base_url}/health", base_url = self.base_url);

        match self.client.get(&url).send().await {
            Ok(response) => Ok(response.status().is_success()),
            Err(e) => {
                debug!("Health check failed: {e}");
                Ok(false)
            }
        }
    }

    pub async fn kill_agent(&self, agent_id: &str) -> Result<()> {
        let url = format!(
            "{base_url}/api/agents/{agent_id}/kill",
            base_url = self.base_url
        );

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .context("Failed to send kill request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Kill request failed with {status}: {text}");
            anyhow::bail!("Kill request failed with {status}");
        }

        info!("Successfully killed agent {agent_id}");
        Ok(())
    }

    pub async fn get_logs(
        &self,
        agent_id: &str,
        _follow: bool,
        tail: Option<usize>,
    ) -> Result<String> {
        let mut url = format!(
            "{base_url}/api/agents/{agent_id}/logs",
            base_url = self.base_url
        );

        if let Some(tail) = tail {
            url = format!("{url}?tail={tail}");
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch logs")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Get logs failed with {status}: {text}");
            anyhow::bail!("Get logs failed with {status}");
        }

        let logs = response.text().await.context("Failed to read logs")?;
        Ok(logs)
    }

    pub async fn get_logs_stream(
        &self,
        agent_id: &str,
        _follow: bool,
        tail: Option<usize>,
    ) -> Result<impl futures::Stream<Item = Result<String>>> {
        // For now, return a simple stream that reads the logs once
        // A full SSE implementation would require a different approach
        let logs = self.get_logs(agent_id, false, tail).await?;

        let stream = futures::stream::once(async move { Ok(logs) });

        Ok(stream)
    }

    pub async fn shutdown(&self) -> Result<()> {
        let url = format!("{base_url}/api/shutdown", base_url = self.base_url);
        info!("Sending shutdown signal to supervisor");

        let response = self
            .client
            .post(&url)
            .send()
            .await
            .context("Failed to send shutdown request")?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            error!("Shutdown request failed with {status}: {text}");
            anyhow::bail!("Shutdown request failed with {status}");
        }

        info!("Supervisor shutdown initiated");
        Ok(())
    }
}
