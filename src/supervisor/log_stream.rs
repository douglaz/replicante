use anyhow::Result;
use std::sync::Arc;
use warp::{Filter, Rejection, Reply, http::StatusCode};

use super::Supervisor;

/// Create a simple log endpoint that returns logs as plain text
pub fn log_stream_route(
    supervisor: Arc<Supervisor>,
) -> impl Filter<Extract = impl Reply, Error = Rejection> + Clone {
    warp::path!("api" / "agents" / String / "logs" / "stream")
        .and(warp::get())
        .and(warp::query::<LogStreamQuery>())
        .and(with_supervisor(supervisor))
        .and_then(handle_log_stream)
}

#[derive(Debug, serde::Deserialize)]
struct LogStreamQuery {
    tail: Option<usize>,
}

async fn handle_log_stream(
    agent_id: String,
    query: LogStreamQuery,
    supervisor: Arc<Supervisor>,
) -> Result<impl Reply, Rejection> {
    // Get container ID for the agent
    let agents_ref = supervisor.get_agents_ref();
    let agents = agents_ref.read().await;

    if let Some(agent) = agents.get(&agent_id) {
        if let Some(container_id) = &agent.container_id {
            let container_id = container_id.clone();
            drop(agents); // Release the lock

            let container_manager = supervisor.get_container_manager_ref();
            let tail_str = query.tail.map(|n| n.to_string());

            // Create log stream from container
            match container_manager
                .stream_container_logs(&container_id, true, tail_str)
                .await
            {
                Ok(mut log_stream) => {
                    use futures::StreamExt;

                    // Collect some logs (for simplicity, not true streaming)
                    let mut logs = String::new();
                    let mut count = 0;
                    while let Some(result) = log_stream.next().await {
                        if count >= 100 {
                            break;
                        } // Limit to 100 lines
                        match result {
                            Ok(log_line) => {
                                logs.push_str(&log_line);
                                logs.push('\n');
                            }
                            Err(e) => {
                                logs.push_str(&format!("Error reading log: {}\n", e));
                                break;
                            }
                        }
                        count += 1;
                    }

                    Ok(warp::reply::with_status(logs, StatusCode::OK))
                }
                Err(e) => Ok(warp::reply::with_status(
                    format!("Failed to get logs: {}", e),
                    StatusCode::INTERNAL_SERVER_ERROR,
                )),
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

fn with_supervisor(
    supervisor: Arc<Supervisor>,
) -> impl Filter<Extract = (Arc<Supervisor>,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || supervisor.clone())
}
