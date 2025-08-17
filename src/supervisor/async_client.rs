use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
pub struct AsyncSupervisorClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnAgentRequest {
    pub config_path: String,
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpawnAgentResponse {
    pub agent_id: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentInfo {
    pub id: String,
    pub status: String,
    pub started_at: String,
    pub container_id: Option<String>,
    pub config_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub agents: Vec<AgentInfo>,
    pub total_agents: usize,
    pub running_agents: usize,
}

impl AsyncSupervisorClient {
    pub fn new(base_url: Option<String>) -> Result<Self> {
        let base_url = base_url.unwrap_or_else(|| {
            std::env::var("SUPERVISOR_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
        });

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self { base_url, client })
    }

    pub async fn spawn_agent(
        &self,
        config_path: &str,
        sandbox_mode: Option<&str>,
    ) -> Result<String> {
        let request = SpawnAgentRequest {
            config_path: config_path.to_string(),
            sandbox_mode: sandbox_mode.map(|s| s.to_string()),
        };

        let response = self
            .client
            .post(format!("{}/api/agents", self.base_url))
            .json(&request)
            .send()
            .await
            .context("Failed to send spawn request")?;

        if !response.status().is_success() {
            let error: String = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            bail!("Failed to spawn agent: {}", error);
        }

        let result: SpawnAgentResponse = response
            .json()
            .await
            .context("Failed to parse spawn response")?;

        Ok(result.agent_id)
    }

    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>> {
        let response = self
            .client
            .get(format!("{}/api/agents", self.base_url))
            .send()
            .await
            .context("Failed to list agents")?;

        if !response.status().is_success() {
            bail!("Failed to list agents: {}", response.status());
        }

        let agents: Vec<AgentInfo> = response
            .json()
            .await
            .context("Failed to parse agents list")?;

        Ok(agents)
    }

    pub async fn get_status(&self) -> Result<StatusResponse> {
        let response = self
            .client
            .get(format!("{}/api/status", self.base_url))
            .send()
            .await
            .context("Failed to get status")?;

        if !response.status().is_success() {
            bail!("Failed to get status: {}", response.status());
        }

        let status: StatusResponse = response.json().await.context("Failed to parse status")?;

        Ok(status)
    }

    pub async fn stop_agent(&self, agent_id: &str) -> Result<()> {
        let response = self
            .client
            .post(format!("{}/api/agents/{}/stop", self.base_url, agent_id))
            .send()
            .await
            .context("Failed to stop agent")?;

        if !response.status().is_success() {
            let error: String = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            bail!("Failed to stop agent: {}", error);
        }

        Ok(())
    }

    pub async fn kill_agent(&self, agent_id: &str) -> Result<()> {
        let response = self
            .client
            .post(format!("{}/api/agents/{}/kill", self.base_url, agent_id))
            .send()
            .await
            .context("Failed to kill agent")?;

        if !response.status().is_success() {
            let error: String = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            bail!("Failed to kill agent: {}", error);
        }

        Ok(())
    }

    pub async fn remove_agent(&self, agent_id: &str) -> Result<()> {
        let response = self
            .client
            .delete(format!("{}/api/agents/{}", self.base_url, agent_id))
            .send()
            .await
            .context("Failed to remove agent")?;

        if !response.status().is_success() {
            let error: String = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            bail!("Failed to remove agent: {}", error);
        }

        Ok(())
    }

    pub async fn get_logs(
        &self,
        agent_id: &str,
        follow: bool,
        tail: Option<usize>,
    ) -> Result<String> {
        let mut url = format!("{}/api/agents/{}/logs", self.base_url, agent_id);

        let mut params = vec![];
        if follow {
            params.push("follow=true".to_string());
        }
        if let Some(tail) = tail {
            params.push(format!("tail={}", tail));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to get logs")?;

        if !response.status().is_success() {
            bail!("Failed to get logs: {}", response.status());
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
        let mut url = format!("{}/api/agents/{}/logs/stream", self.base_url, agent_id);

        let mut params = vec![];
        if let Some(tail) = tail {
            params.push(format!("tail={}", tail));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        // For now, return a simple stream that reads the logs once
        // A full SSE implementation would require a different approach
        let logs = self.get_logs(agent_id, false, tail).await?;

        let stream = futures::stream::once(async move { Ok(logs) });

        Ok(stream)
    }
}
