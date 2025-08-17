use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};
use warp::{Filter, Rejection, Reply, http::StatusCode};

use super::{
    AgentProcess, FilesystemRestrictions, MCPRestrictions, Monitor, NetworkMode,
    NetworkRestrictions, ResourceLimits, SandboxConfig, SandboxMode, Supervisor,
};

#[derive(Debug, Serialize, Deserialize)]
struct StatusResponse {
    agents: Vec<AgentInfo>,
    total_agents: usize,
    running_agents: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct AgentInfo {
    id: String,
    status: String,
    started_at: String,
    resource_usage: super::ResourceUsage,
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricsResponse {
    metrics: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct EventsResponse {
    events: Vec<super::monitor::Event>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AlertsResponse {
    alerts: Vec<super::monitor::Alert>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SpawnAgentRequest {
    config_path: String,
    sandbox_mode: Option<String>, // "strict", "moderate", "permissive"
}

#[derive(Debug, Serialize, Deserialize)]
struct SpawnAgentResponse {
    agent_id: String,
    status: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct AgentActionResponse {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
}

pub async fn start_dashboard_server(port: u16, supervisor: Arc<Supervisor>) -> Result<()> {
    info!("Starting dashboard server on port {port}");

    // Get references from supervisor
    let agents = supervisor.get_agents_ref();
    let monitor = supervisor.get_monitor_ref();

    // Clone for move into async blocks
    let agents_clone = agents.clone();
    let monitor_clone = monitor.clone();
    let supervisor_clone = supervisor.clone();

    // Status endpoint
    let status = warp::path("api")
        .and(warp::path("status"))
        .and(warp::get())
        .and(with_agents(agents_clone.clone()))
        .and_then(handle_status);

    // Metrics endpoint
    let metrics = warp::path("api")
        .and(warp::path("metrics"))
        .and(warp::get())
        .and(with_monitor(monitor_clone.clone()))
        .and_then(handle_metrics);

    // Events endpoint
    let events = warp::path("api")
        .and(warp::path("events"))
        .and(warp::get())
        .and(with_monitor(monitor_clone.clone()))
        .and_then(handle_events);

    // Alerts endpoint
    let alerts = warp::path("api")
        .and(warp::path("alerts"))
        .and(warp::get())
        .and(with_monitor(monitor_clone.clone()))
        .and_then(handle_alerts);

    // Agent management endpoints

    // POST /api/agents - Spawn new agent
    let spawn_agent = warp::path("api")
        .and(warp::path("agents"))
        .and(warp::post())
        .and(warp::body::json())
        .and(with_supervisor(supervisor_clone.clone()))
        .and_then(handle_spawn_agent);

    // GET /api/agents - List all agents
    let list_agents = warp::path("api")
        .and(warp::path("agents"))
        .and(warp::get())
        .and(with_agents(agents_clone.clone()))
        .and_then(handle_list_agents);

    // POST /api/agents/{id}/stop - Stop agent
    let stop_agent = warp::path("api")
        .and(warp::path("agents"))
        .and(warp::path::param::<String>())
        .and(warp::path("stop"))
        .and(warp::post())
        .and(with_supervisor(supervisor_clone.clone()))
        .and_then(handle_stop_agent);

    // POST /api/agents/{id}/kill - Kill agent
    let kill_agent = warp::path("api")
        .and(warp::path("agents"))
        .and(warp::path::param::<String>())
        .and(warp::path("kill"))
        .and(warp::post())
        .and(with_supervisor(supervisor_clone.clone()))
        .and_then(handle_kill_agent);

    // DELETE /api/agents/{id} - Remove agent
    let remove_agent = warp::path("api")
        .and(warp::path("agents"))
        .and(warp::path::param::<String>())
        .and(warp::delete())
        .and(with_supervisor(supervisor_clone.clone()))
        .and_then(handle_remove_agent);

    // GET /api/agents/{id}/logs - Get agent logs
    let agent_logs = warp::path("api")
        .and(warp::path("agents"))
        .and(warp::path::param::<String>())
        .and(warp::path("logs"))
        .and(warp::get())
        .and(warp::query::<LogsQuery>())
        .and(with_supervisor(supervisor_clone.clone()))
        .and_then(handle_agent_logs);

    // GET /api/agents/{id}/logs/stream - Stream agent logs (SSE)
    let log_stream = super::log_stream::log_stream_route(supervisor_clone.clone());

    // Static files for dashboard UI
    let dashboard = warp::path::end()
        .and(warp::get())
        .map(|| warp::reply::html(DASHBOARD_HTML));

    // Combine all routes
    let routes = status
        .or(metrics)
        .or(events)
        .or(alerts)
        .or(spawn_agent)
        .or(list_agents)
        .or(stop_agent)
        .or(kill_agent)
        .or(remove_agent)
        .or(agent_logs)
        .or(log_stream)
        .or(dashboard)
        .with(warp::cors().allow_any_origin());

    // Start server
    tokio::spawn(async move {
        warp::serve(routes).run(([0, 0, 0, 0], port)).await;
    });

    Ok(())
}

fn with_agents(
    agents: Arc<RwLock<HashMap<String, AgentProcess>>>,
) -> impl Filter<
    Extract = (Arc<RwLock<HashMap<String, AgentProcess>>>,),
    Error = std::convert::Infallible,
> + Clone {
    warp::any().map(move || agents.clone())
}

fn with_monitor(
    monitor: Arc<Monitor>,
) -> impl Filter<Extract = (Arc<Monitor>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || monitor.clone())
}

fn with_supervisor(
    supervisor: Arc<Supervisor>,
) -> impl Filter<Extract = (Arc<Supervisor>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || supervisor.clone())
}

#[derive(Debug, Deserialize)]
struct LogsQuery {
    follow: Option<bool>,
    tail: Option<usize>,
}

async fn handle_status(
    agents: Arc<RwLock<HashMap<String, AgentProcess>>>,
) -> Result<impl Reply, Rejection> {
    let agents_guard = agents.read().await;

    let agent_list: Vec<AgentInfo> = agents_guard
        .values()
        .map(|agent| AgentInfo {
            id: agent.id.clone(),
            status: format!("{:?}", agent.status),
            started_at: agent.started_at.to_rfc3339(),
            resource_usage: agent.resource_usage.clone(),
        })
        .collect();

    let running_count = agents_guard
        .values()
        .filter(|a| matches!(a.status, super::AgentStatus::Running))
        .count();

    let response = StatusResponse {
        total_agents: agent_list.len(),
        running_agents: running_count,
        agents: agent_list,
    };

    Ok(warp::reply::json(&response))
}

async fn handle_metrics(monitor: Arc<Monitor>) -> Result<impl Reply, Rejection> {
    let metrics_data = monitor.export_metrics("json").await.map_err(|e| {
        error!("Failed to export metrics: {e}");
        warp::reject::reject()
    })?;

    let response = MetricsResponse {
        metrics: serde_json::from_str(&metrics_data).unwrap_or(serde_json::json!({})),
    };

    Ok(warp::reply::json(&response))
}

async fn handle_events(monitor: Arc<Monitor>) -> Result<impl Reply, Rejection> {
    let events = monitor.get_recent_events(100).await;

    let response = EventsResponse { events };

    Ok(warp::reply::json(&response))
}

async fn handle_alerts(monitor: Arc<Monitor>) -> Result<impl Reply, Rejection> {
    let alerts = monitor.get_recent_alerts(50).await;

    let response = AlertsResponse { alerts };

    Ok(warp::reply::json(&response))
}

async fn handle_spawn_agent(
    request: SpawnAgentRequest,
    supervisor: Arc<Supervisor>,
) -> Result<warp::reply::WithStatus<warp::reply::Json>, Rejection> {
    // Parse sandbox mode if provided
    let sandbox_config = if let Some(mode_str) = request.sandbox_mode {
        let mode = match mode_str.to_lowercase().as_str() {
            "strict" => SandboxMode::Strict,
            "moderate" => SandboxMode::Moderate,
            "permissive" => SandboxMode::Permissive,
            _ => SandboxMode::Moderate,
        };

        Some(SandboxConfig {
            enabled: true,
            mode,
            filesystem: FilesystemRestrictions {
                root: "/app".to_string(),
                read_only_paths: vec![],
                write_paths: vec!["/app/data".to_string()],
                max_size_mb: 100,
            },
            network: NetworkRestrictions {
                mode: NetworkMode::Filtered,
                allowed_domains: vec![],
                blocked_ports: vec![],
                rate_limit_per_minute: Some(100),
            },
            resources: ResourceLimits {
                max_memory_mb: 512,
                max_cpu_percent: 50.0,
                max_processes: 10,
                max_open_files: 100,
            },
            mcp: MCPRestrictions {
                allowed_servers: vec![],
                blocked_tools: vec![],
                tool_rate_limits: HashMap::new(),
            },
        })
    } else {
        None
    };

    match supervisor
        .spawn_agent(request.config_path, sandbox_config)
        .await
    {
        Ok(agent_id) => {
            let response = SpawnAgentResponse {
                agent_id,
                status: "Starting".to_string(),
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            error!("Failed to spawn agent: {e}");
            let error_response = ErrorResponse {
                error: e.to_string(),
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&error_response),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

async fn handle_list_agents(
    agents: Arc<RwLock<HashMap<String, AgentProcess>>>,
) -> Result<impl Reply, Rejection> {
    let agents_guard = agents.read().await;

    let agent_list: Vec<super::client::AgentInfo> = agents_guard
        .values()
        .map(|agent| super::client::AgentInfo {
            id: agent.id.clone(),
            status: format!("{:?}", agent.status),
            started_at: agent.started_at.to_rfc3339(),
            container_id: agent.container_id.clone(),
            config_path: agent.config_path.clone(),
        })
        .collect();

    Ok(warp::reply::json(&agent_list))
}

async fn handle_stop_agent(
    agent_id: String,
    supervisor: Arc<Supervisor>,
) -> Result<warp::reply::WithStatus<warp::reply::Json>, Rejection> {
    match supervisor.stop_agent(&agent_id).await {
        Ok(_) => {
            let response = AgentActionResponse {
                success: true,
                message: format!("Agent {} stopped successfully", agent_id),
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            error!("Failed to stop agent {agent_id}: {e}");
            let error_response = ErrorResponse {
                error: e.to_string(),
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&error_response),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

async fn handle_kill_agent(
    agent_id: String,
    supervisor: Arc<Supervisor>,
) -> Result<warp::reply::WithStatus<warp::reply::Json>, Rejection> {
    match supervisor.emergency_stop(&agent_id).await {
        Ok(_) => {
            let response = AgentActionResponse {
                success: true,
                message: format!("Agent {} killed successfully", agent_id),
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&response),
                StatusCode::OK,
            ))
        }
        Err(e) => {
            error!("Failed to kill agent {agent_id}: {e}");
            let error_response = ErrorResponse {
                error: e.to_string(),
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&error_response),
                StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

async fn handle_remove_agent(
    agent_id: String,
    supervisor: Arc<Supervisor>,
) -> Result<warp::reply::WithStatus<warp::reply::Json>, Rejection> {
    // First stop the agent if running
    let _ = supervisor.stop_agent(&agent_id).await;

    // Remove from agents map
    let agents_ref = supervisor.get_agents_ref();
    let mut agents = agents_ref.write().await;
    if agents.remove(&agent_id).is_some() {
        let response = AgentActionResponse {
            success: true,
            message: format!("Agent {} removed successfully", agent_id),
        };
        Ok(warp::reply::with_status(
            warp::reply::json(&response),
            StatusCode::OK,
        ))
    } else {
        let error_response = ErrorResponse {
            error: format!("Agent {} not found", agent_id),
        };
        Ok(warp::reply::with_status(
            warp::reply::json(&error_response),
            StatusCode::NOT_FOUND,
        ))
    }
}

async fn handle_agent_logs(
    agent_id: String,
    query: LogsQuery,
    supervisor: Arc<Supervisor>,
) -> Result<impl Reply, Rejection> {
    // Get container ID and manager reference for the agent
    let agents_ref = supervisor.get_agents_ref();
    let agents = agents_ref.read().await;

    if let Some(agent) = agents.get(&agent_id) {
        if let Some(container_id) = &agent.container_id {
            let container_id = container_id.clone();
            drop(agents); // Release the lock

            // Get container manager reference
            let container_manager = supervisor.get_container_manager_ref();

            // Fetch logs from container
            let follow = query.follow.unwrap_or(false);
            let tail = query.tail.map(|n| n.to_string());

            if follow {
                // For streaming logs, we need to return a stream response
                // This is a simplified version - in production you'd use Server-Sent Events or WebSocket
                use futures::StreamExt;

                match container_manager
                    .stream_container_logs(&container_id, true, tail)
                    .await
                {
                    Ok(mut stream) => {
                        let mut logs = String::new();
                        // Collect first batch of logs (for simplicity in this implementation)
                        for _ in 0..100 {
                            if let Some(Ok(log)) = stream.next().await {
                                logs.push_str(&log);
                                logs.push('\n');
                            } else {
                                break;
                            }
                        }
                        Ok(warp::reply::with_status(logs, StatusCode::OK))
                    }
                    Err(e) => Ok(warp::reply::with_status(
                        format!("Failed to fetch logs: {}", e),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )),
                }
            } else {
                // For non-streaming, fetch all available logs
                match container_manager
                    .stream_container_logs(&container_id, false, tail)
                    .await
                {
                    Ok(mut stream) => {
                        use futures::StreamExt;
                        let mut logs = String::new();
                        while let Some(result) = stream.next().await {
                            match result {
                                Ok(log) => {
                                    logs.push_str(&log);
                                    logs.push('\n');
                                }
                                Err(e) => {
                                    error!("Error reading log: {}", e);
                                    break;
                                }
                            }
                        }
                        Ok(warp::reply::with_status(logs, StatusCode::OK))
                    }
                    Err(e) => Ok(warp::reply::with_status(
                        format!("Failed to fetch logs: {}", e),
                        StatusCode::INTERNAL_SERVER_ERROR,
                    )),
                }
            }
        } else {
            Ok(warp::reply::with_status(
                format!("No container found for agent {}", agent_id),
                StatusCode::NOT_FOUND,
            ))
        }
    } else {
        Ok(warp::reply::with_status(
            format!("Agent {} not found", agent_id),
            StatusCode::NOT_FOUND,
        ))
    }
}

// Basic dashboard HTML
const DASHBOARD_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Replicante Supervisor Dashboard</title>
    <style>
        * { margin: 0; padding: 0; box-sizing: border-box; }
        body { 
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #0f0f0f;
            color: #e0e0e0;
            padding: 20px;
        }
        .container { max-width: 1400px; margin: 0 auto; }
        h1 { 
            color: #00ff88;
            margin-bottom: 30px;
            font-size: 2em;
            text-shadow: 0 0 10px rgba(0, 255, 136, 0.5);
        }
        .grid { 
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }
        .card {
            background: #1a1a1a;
            border: 1px solid #333;
            border-radius: 8px;
            padding: 20px;
            box-shadow: 0 4px 6px rgba(0, 0, 0, 0.3);
        }
        .card h2 {
            color: #00ff88;
            font-size: 1.2em;
            margin-bottom: 15px;
        }
        .status { 
            display: inline-block;
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 0.9em;
            font-weight: 500;
        }
        .status.running { background: #00ff88; color: #000; }
        .status.stopped { background: #ff4444; color: #fff; }
        .status.starting { background: #ffaa00; color: #000; }
        .metric {
            display: flex;
            justify-content: space-between;
            margin: 10px 0;
            padding: 8px;
            background: #0f0f0f;
            border-radius: 4px;
        }
        .metric-label { color: #888; }
        .metric-value { 
            color: #00ff88;
            font-weight: bold;
            font-family: 'Courier New', monospace;
        }
        .alerts {
            max-height: 300px;
            overflow-y: auto;
        }
        .alert {
            padding: 10px;
            margin: 5px 0;
            background: #2a1a1a;
            border-left: 3px solid #ff4444;
            border-radius: 4px;
        }
        .events {
            max-height: 400px;
            overflow-y: auto;
        }
        .event {
            padding: 8px;
            margin: 5px 0;
            background: #0f0f0f;
            border-radius: 4px;
            font-size: 0.9em;
        }
        .timestamp {
            color: #666;
            font-size: 0.85em;
        }
        button {
            background: #00ff88;
            color: #000;
            border: none;
            padding: 10px 20px;
            border-radius: 4px;
            font-weight: bold;
            cursor: pointer;
            margin: 5px;
        }
        button:hover {
            background: #00cc66;
        }
        button.danger {
            background: #ff4444;
            color: #fff;
        }
        button.danger:hover {
            background: #cc0000;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🤖 Replicante Supervisor</h1>
        
        <div class="grid">
            <div class="card">
                <h2>System Status</h2>
                <div id="system-status">
                    <div class="metric">
                        <span class="metric-label">Total Agents:</span>
                        <span class="metric-value" id="total-agents">0</span>
                    </div>
                    <div class="metric">
                        <span class="metric-label">Running:</span>
                        <span class="metric-value" id="running-agents">0</span>
                    </div>
                </div>
            </div>
            
            <div class="card">
                <h2>Agents</h2>
                <div id="agents-list"></div>
            </div>
            
            <div class="card">
                <h2>Recent Alerts</h2>
                <div class="alerts" id="alerts-list"></div>
            </div>
        </div>
        
        <div class="card">
            <h2>Recent Events</h2>
            <div class="events" id="events-list"></div>
        </div>
        
        <div class="card">
            <h2>Controls</h2>
            <button onclick="refreshData()">Refresh</button>
            <button class="danger" onclick="emergencyStopAll()">Emergency Stop All</button>
        </div>
    </div>

    <script>
        async function fetchData() {
            try {
                // Fetch status
                const statusRes = await fetch('/api/status');
                const status = await statusRes.json();
                
                document.getElementById('total-agents').textContent = status.total_agents;
                document.getElementById('running-agents').textContent = status.running_agents;
                
                // Update agents list
                const agentsList = document.getElementById('agents-list');
                agentsList.innerHTML = status.agents.map(agent => `
                    <div class="metric">
                        <span>${agent.id}</span>
                        <span class="status ${agent.status.toLowerCase()}">${agent.status}</span>
                    </div>
                `).join('');
                
                // Fetch alerts
                const alertsRes = await fetch('/api/alerts');
                const alertsData = await alertsRes.json();
                
                const alertsList = document.getElementById('alerts-list');
                alertsList.innerHTML = alertsData.alerts.slice(0, 5).map(alert => `
                    <div class="alert">
                        ${JSON.stringify(alert)}
                    </div>
                `).join('') || '<div class="event">No recent alerts</div>';
                
                // Fetch events
                const eventsRes = await fetch('/api/events');
                const eventsData = await eventsRes.json();
                
                const eventsList = document.getElementById('events-list');
                eventsList.innerHTML = eventsData.events.slice(0, 10).map(event => `
                    <div class="event">
                        <span class="timestamp">${new Date(event.timestamp).toLocaleString()}</span>
                        <strong>${event.agent_id}</strong> - ${event.event_type}
                    </div>
                `).join('') || '<div class="event">No recent events</div>';
                
            } catch (error) {
                console.error('Failed to fetch data:', error);
            }
        }
        
        function refreshData() {
            fetchData();
        }
        
        function emergencyStopAll() {
            if (confirm('Are you sure you want to stop all agents?')) {
                // TODO: Implement emergency stop API call
                alert('Emergency stop not yet implemented');
            }
        }
        
        // Auto-refresh every 5 seconds
        setInterval(fetchData, 5000);
        
        // Initial load
        fetchData();
    </script>
</body>
</html>
"#;
