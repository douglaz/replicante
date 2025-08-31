use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::{AgentProcess, ResourceUsage};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Alert {
    HighResourceUsage {
        agent_id: String,
        metric: String,
        value: f64,
        threshold: f64,
    },
    SuspiciousToolUsage {
        agent_id: String,
        tool: String,
        frequency: u32,
    },
    UnauthorizedAccess {
        agent_id: String,
        path: String,
    },
    NetworkAnomaly {
        agent_id: String,
        destination: String,
    },
    PrivilegeEscalation {
        agent_id: String,
        attempt: String,
    },
    AgentCrashed {
        agent_id: String,
        exit_code: Option<i32>,
    },
    AgentQuarantined {
        agent_id: String,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub timestamp: DateTime<Utc>,
    pub agent_id: String,
    pub event_type: EventType,
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    AgentStarted,
    AgentStopped,
    ToolUsed,
    Decision,
    Error,
    Alert,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub timestamp: DateTime<Utc>,
    pub agent_id: String,
    pub cpu_percent: f64,
    pub memory_mb: u64,
    pub disk_io_bytes: u64,
    pub network_bytes: u64,
    pub tool_calls: u32,
}

pub struct Monitor {
    events: Arc<RwLock<VecDeque<Event>>>,
    metrics: Arc<RwLock<HashMap<String, VecDeque<Metrics>>>>,
    alerts: Arc<RwLock<VecDeque<Alert>>>,
    max_events: usize,
    max_metrics_per_agent: usize,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(VecDeque::new())),
            metrics: Arc::new(RwLock::new(HashMap::new())),
            alerts: Arc::new(RwLock::new(VecDeque::new())),
            max_events: 10000,
            max_metrics_per_agent: 1000,
        }
    }

    pub async fn start_monitoring(&self, agent_id: &str) -> Result<()> {
        info!("Starting monitoring for agent {agent_id}");

        // Initialize metrics storage for this agent
        let mut metrics = self.metrics.write().await;
        metrics.insert(agent_id.to_string(), VecDeque::new());

        // Log event
        self.log_event(Event {
            timestamp: Utc::now(),
            agent_id: agent_id.to_string(),
            event_type: EventType::AgentStarted,
            details: serde_json::json!({}),
        })
        .await;

        Ok(())
    }

    pub async fn stop_monitoring(&self, agent_id: &str) -> Result<()> {
        info!("Stopping monitoring for agent {agent_id}");

        // Log event
        self.log_event(Event {
            timestamp: Utc::now(),
            agent_id: agent_id.to_string(),
            event_type: EventType::AgentStopped,
            details: serde_json::json!({}),
        })
        .await;

        Ok(())
    }

    pub async fn check_agent_health(&self, agent: &AgentProcess) -> Result<()> {
        debug!("Checking health for agent {id}", id = agent.id);

        // Check resource usage
        if agent.resource_usage.cpu_percent > 80.0 {
            self.alert(Alert::HighResourceUsage {
                agent_id: agent.id.clone(),
                metric: "CPU".to_string(),
                value: agent.resource_usage.cpu_percent,
                threshold: 80.0,
            })
            .await?;
        }

        if agent.resource_usage.memory_mb > 512 {
            self.alert(Alert::HighResourceUsage {
                agent_id: agent.id.clone(),
                metric: "Memory".to_string(),
                value: agent.resource_usage.memory_mb as f64,
                threshold: 512.0,
            })
            .await?;
        }

        // Check tool usage patterns
        for (tool, count) in &agent.tool_usage {
            if *count > 100 {
                self.alert(Alert::SuspiciousToolUsage {
                    agent_id: agent.id.clone(),
                    tool: tool.clone(),
                    frequency: *count,
                })
                .await?;
            }
        }

        Ok(())
    }

    pub async fn record_metrics(
        &self,
        agent_id: &str,
        resource_usage: ResourceUsage,
    ) -> Result<()> {
        let metrics_entry = Metrics {
            timestamp: Utc::now(),
            agent_id: agent_id.to_string(),
            cpu_percent: resource_usage.cpu_percent,
            memory_mb: resource_usage.memory_mb,
            disk_io_bytes: resource_usage.disk_io_bytes,
            network_bytes: resource_usage.network_bytes,
            tool_calls: 0, // Will be updated separately
        };

        let mut metrics = self.metrics.write().await;
        if let Some(agent_metrics) = metrics.get_mut(agent_id) {
            agent_metrics.push_back(metrics_entry);

            // Keep only recent metrics
            while agent_metrics.len() > self.max_metrics_per_agent {
                agent_metrics.pop_front();
            }
        }

        Ok(())
    }

    pub async fn log_event(&self, event: Event) {
        let mut events = self.events.write().await;
        events.push_back(event);

        // Keep only recent events
        while events.len() > self.max_events {
            events.pop_front();
        }
    }

    pub async fn alert(&self, alert: Alert) -> Result<()> {
        warn!("Alert: {:?}", alert);

        let mut alerts = self.alerts.write().await;
        alerts.push_back(alert.clone());

        // Keep only recent alerts
        while alerts.len() > 1000 {
            alerts.pop_front();
        }

        // Log as event too
        self.log_event(Event {
            timestamp: Utc::now(),
            agent_id: match &alert {
                Alert::HighResourceUsage { agent_id, .. }
                | Alert::SuspiciousToolUsage { agent_id, .. }
                | Alert::UnauthorizedAccess { agent_id, .. }
                | Alert::NetworkAnomaly { agent_id, .. }
                | Alert::PrivilegeEscalation { agent_id, .. }
                | Alert::AgentCrashed { agent_id, .. }
                | Alert::AgentQuarantined { agent_id, .. } => agent_id.clone(),
            },
            event_type: EventType::Alert,
            details: serde_json::to_value(&alert)?,
        })
        .await;

        Ok(())
    }

    pub async fn get_recent_events(&self, limit: usize) -> Vec<Event> {
        let events = self.events.read().await;
        events.iter().rev().take(limit).cloned().collect()
    }

    pub async fn get_recent_alerts(&self, limit: usize) -> Vec<Alert> {
        let alerts = self.alerts.read().await;
        alerts.iter().rev().take(limit).cloned().collect()
    }

    pub async fn get_agent_metrics(&self, agent_id: &str) -> Option<Vec<Metrics>> {
        let metrics = self.metrics.read().await;
        metrics.get(agent_id).map(|m| m.iter().cloned().collect())
    }

    pub async fn generate_incident_report(&self, agent_id: &str) -> Result<()> {
        info!("Generating incident report for agent {agent_id}");

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("incident_{agent_id}_{timestamp}.json");

        // Collect all relevant data
        let events = self.events.read().await;
        let agent_events: Vec<_> = events
            .iter()
            .filter(|e| e.agent_id == agent_id)
            .cloned()
            .collect();

        let alerts = self.alerts.read().await;
        let agent_alerts: Vec<_> = alerts
            .iter()
            .filter(|a| match a {
                Alert::HighResourceUsage { agent_id: id, .. }
                | Alert::SuspiciousToolUsage { agent_id: id, .. }
                | Alert::UnauthorizedAccess { agent_id: id, .. }
                | Alert::NetworkAnomaly { agent_id: id, .. }
                | Alert::PrivilegeEscalation { agent_id: id, .. }
                | Alert::AgentCrashed { agent_id: id, .. }
                | Alert::AgentQuarantined { agent_id: id, .. } => id == agent_id,
            })
            .cloned()
            .collect();

        let metrics = self.metrics.read().await;
        let agent_metrics = metrics.get(agent_id).cloned();

        let report = serde_json::json!({
            "incident_id": Uuid::new_v4().to_string(),
            "timestamp": Utc::now(),
            "agent_id": agent_id,
            "events": agent_events,
            "alerts": agent_alerts,
            "metrics": agent_metrics,
        });

        // Write to file
        let mut file = File::create(&filename)
            .await
            .context("Failed to create incident report file")?;

        let json = serde_json::to_string_pretty(&report)?;
        file.write_all(json.as_bytes())
            .await
            .context("Failed to write incident report")?;

        info!("Incident report saved to {filename}");

        Ok(())
    }

    pub async fn export_metrics(&self, format: &str) -> Result<String> {
        match format {
            "json" => {
                let metrics = self.metrics.read().await;
                Ok(serde_json::to_string_pretty(&*metrics)?)
            }
            "prometheus" => {
                let mut output = String::new();
                let metrics = self.metrics.read().await;

                for (agent_id, agent_metrics) in metrics.iter() {
                    if let Some(latest) = agent_metrics.back() {
                        output.push_str(&format!(
                            "# HELP agent_cpu_percent CPU usage percentage\n\
                             # TYPE agent_cpu_percent gauge\n\
                             agent_cpu_percent{{agent_id=\"{}\"}} {}\n\
                             # HELP agent_memory_mb Memory usage in MB\n\
                             # TYPE agent_memory_mb gauge\n\
                             agent_memory_mb{{agent_id=\"{}\"}} {}\n",
                            agent_id, latest.cpu_percent, agent_id, latest.memory_mb
                        ));
                    }
                }

                Ok(output)
            }
            _ => anyhow::bail!("Unsupported format: {}", format),
        }
    }
}

use uuid::Uuid;
