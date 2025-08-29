use anyhow::Result;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tracing::{error, info};

use super::{AgentProcess, Monitor};

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

#[derive(Clone)]
struct AppState {
    agents: Arc<RwLock<HashMap<String, AgentProcess>>>,
    monitor: Arc<Monitor>,
}

pub async fn start_dashboard_server(
    port: u16,
    agents: Arc<RwLock<HashMap<String, AgentProcess>>>,
    monitor: Arc<Monitor>,
) -> Result<()> {
    info!("Starting dashboard server on port {port}");

    let state = AppState { agents, monitor };

    let app = Router::new()
        .route("/api/status", get(handle_status))
        .route("/api/metrics", get(handle_metrics))
        .route("/api/events", get(handle_events))
        .route("/api/alerts", get(handle_alerts))
        .route("/api/shutdown", post(handle_shutdown))
        .route("/", get(handle_dashboard))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("Failed to bind address");

        axum::serve(listener, app)
            .await
            .expect("Failed to start server");
    });

    Ok(())
}

async fn handle_status(State(state): State<AppState>) -> impl IntoResponse {
    let agents_guard = state.agents.read().await;

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

    Json(response)
}

async fn handle_metrics(State(state): State<AppState>) -> Result<impl IntoResponse, AppError> {
    let metrics_data = state.monitor.export_metrics("json").await.map_err(|e| {
        error!("Failed to export metrics: {e}");
        AppError::InternalError
    })?;

    let response = MetricsResponse {
        metrics: serde_json::from_str(&metrics_data).unwrap_or(serde_json::json!({})),
    };

    Ok(Json(response))
}

async fn handle_events(State(state): State<AppState>) -> impl IntoResponse {
    let events = state.monitor.get_recent_events(100).await;
    let response = EventsResponse { events };
    Json(response)
}

async fn handle_alerts(State(state): State<AppState>) -> impl IntoResponse {
    let alerts = state.monitor.get_recent_alerts(50).await;
    let response = AlertsResponse { alerts };
    Json(response)
}

async fn handle_shutdown() -> impl IntoResponse {
    info!("Shutdown request received");

    // Send shutdown signal after response
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        std::process::exit(0);
    });

    Json(serde_json::json!({"status": "shutdown initiated"}))
}

async fn handle_dashboard() -> impl IntoResponse {
    Html(DASHBOARD_HTML)
}

// Custom error type for better error handling
enum AppError {
    InternalError,
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            AppError::InternalError => (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error"),
        };

        let body = Json(serde_json::json!({
            "error": error_message,
        }));

        (status, body).into_response()
    }
}

// Basic dashboard HTML (same as before)
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
