use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

pub struct LogStreamer {
    container_id: String,
}

impl LogStreamer {
    pub fn new(container_id: String) -> Self {
        Self { container_id }
    }

    pub async fn stream_logs(&self, tx: mpsc::Sender<String>) -> Result<()> {
        info!(
            "Starting log stream for container {container_id}",
            container_id = self.container_id
        );

        // Verify container exists first
        if !self.container_exists().await? {
            bail!("Container {} does not exist", self.container_id);
        }

        let mut cmd = Command::new("docker");
        cmd.arg("logs")
            .arg("--follow")
            .arg("--tail")
            .arg("100")
            .arg(&self.container_id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().context("Failed to spawn docker logs command")?;

        // Get stdout and stderr
        let stdout = child
            .stdout
            .take()
            .context("Failed to get stdout from docker logs")?;
        let stderr = child
            .stderr
            .take()
            .context("Failed to get stderr from docker logs")?;

        // Create readers
        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        // Clone tx for stderr task
        let tx_stderr = tx.clone();
        let container_id = self.container_id.clone();

        // Spawn stdout reader task
        let stdout_task = tokio::spawn(async move {
            let mut lines = stdout_reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Err(e) = tx.send(format!("[STDOUT] {line}")).await {
                    warn!("Failed to send stdout log line: {e}");
                    break;
                }
            }
            debug!("Stdout stream ended for container {container_id}");
        });

        // Spawn stderr reader task
        let container_id = self.container_id.clone();
        let stderr_task = tokio::spawn(async move {
            let mut lines = stderr_reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Err(e) = tx_stderr.send(format!("[STDERR] {line}")).await {
                    warn!("Failed to send stderr log line: {e}");
                    break;
                }
            }
            debug!("Stderr stream ended for container {container_id}");
        });

        // Wait for both tasks to complete
        let _ = tokio::join!(stdout_task, stderr_task);

        // Wait for the child process to exit
        let status = child
            .wait()
            .await
            .context("Failed to wait for docker logs process")?;

        if !status.success() {
            error!("Docker logs command failed with status: {status}");
        }

        info!(
            "Log stream ended for container {container_id}",
            container_id = self.container_id
        );
        Ok(())
    }

    pub async fn get_recent_logs(&self, lines: usize) -> Result<Vec<String>> {
        debug!(
            "Getting last {} logs for container {}",
            lines, self.container_id
        );

        // Verify container exists first
        if !self.container_exists().await? {
            bail!("Container {} does not exist", self.container_id);
        }

        let output = Command::new("docker")
            .arg("logs")
            .arg("--tail")
            .arg(lines.to_string())
            .arg(&self.container_id)
            .output()
            .await
            .context("Failed to execute docker logs command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to get logs: {stderr}");
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut logs = Vec::new();

        // Add stdout lines
        for line in stdout.lines() {
            if !line.is_empty() {
                logs.push(format!("[STDOUT] {line}"));
            }
        }

        // Add stderr lines
        for line in stderr.lines() {
            if !line.is_empty() {
                logs.push(format!("[STDERR] {line}"));
            }
        }

        Ok(logs)
    }

    async fn container_exists(&self) -> Result<bool> {
        let output = Command::new("docker")
            .arg("inspect")
            .arg(&self.container_id)
            .arg("--format")
            .arg("{{.State.Status}}")
            .output()
            .await
            .context("Failed to inspect container")?;

        Ok(output.status.success())
    }

    pub async fn parse_json_logs(&self, lines: usize) -> Result<Vec<Value>> {
        let logs = self.get_recent_logs(lines).await?;
        let mut json_logs = Vec::new();

        for log in logs {
            // Try to extract JSON from the log line
            if let Some(json_start) = log.find('{') {
                let json_str = &log[json_start..];
                if let Ok(value) = serde_json::from_str::<Value>(json_str) {
                    json_logs.push(value);
                }
            }
        }

        Ok(json_logs)
    }
}
