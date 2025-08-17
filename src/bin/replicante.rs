use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;

// Import the main modules
use replicante::{run_agent, run_sandboxed, supervisor};

#[derive(Parser)]
#[command(name = "replicante")]
#[command(about = "Autonomous AI Agent with Supervisor and Sandbox", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the autonomous agent (default mode)
    Agent {
        /// Path to configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,
    },

    /// Run the supervisor daemon
    Supervisor {
        #[command(subcommand)]
        command: SupervisorCommands,
    },

    /// Run agent in sandboxed environment (Docker container)
    Sandbox {
        /// Path to agent configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Connect to supervisor at this address
        #[arg(long)]
        supervisor: Option<String>,
    },

    /// Monitor running agents
    Monitor {
        #[command(subcommand)]
        command: MonitorCommands,
    },
}

#[derive(Subcommand)]
enum SupervisorCommands {
    /// Start the supervisor daemon
    Start {
        /// Path to supervisor configuration file
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Web dashboard port
        #[arg(short = 'p', long)]
        web_port: Option<u16>,
    },

    /// Show supervisor status
    Status,

    /// Stop an agent
    Stop {
        /// Agent ID to stop
        agent_id: String,
    },

    /// Emergency stop an agent
    Kill {
        /// Agent ID to kill
        agent_id: String,
    },

    /// Quarantine an agent
    Quarantine {
        /// Agent ID to quarantine
        agent_id: String,
    },

    /// View agent logs
    Logs {
        /// Agent ID
        agent_id: String,

        /// Follow log output
        #[arg(short, long)]
        follow: bool,
    },
}

#[derive(Subcommand)]
enum MonitorCommands {
    /// Show metrics for an agent
    Metrics {
        /// Agent ID (optional, shows all if not specified)
        agent_id: Option<String>,

        /// Output format (json, prometheus)
        #[arg(short, long, default_value = "json")]
        format: String,
    },

    /// Show recent events
    Events {
        /// Number of events to show
        #[arg(short = 'n', long, default_value = "50")]
        limit: usize,
    },

    /// Show recent alerts
    Alerts {
        /// Number of alerts to show
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,
    },

    /// Show agent decision history
    Decisions {
        /// Agent ID
        agent_id: String,

        /// Number of decisions to show
        #[arg(short = 'n', long, default_value = "100")]
        last: usize,
    },

    /// Export audit log
    Audit {
        /// Export to file
        #[arg(long)]
        export: Option<PathBuf>,
    },

    /// Open web dashboard
    Dashboard {
        /// Dashboard URL
        #[arg(default_value = "http://localhost:8080")]
        url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Agent { config } => {
            info!("Starting Replicante agent");
            run_agent(config).await?;
        }

        Commands::Supervisor { command } => match command {
            SupervisorCommands::Start { config, web_port } => {
                info!("Starting supervisor daemon");

                let mut supervisor_config = if let Some(ref path) = config {
                    let contents = tokio::fs::read_to_string(path).await?;
                    toml::from_str(&contents)?
                } else {
                    supervisor::SupervisorConfig::default()
                };

                if let Some(port) = web_port {
                    supervisor_config.web_port = Some(port);
                }

                let daemon = supervisor::daemon::Daemon::new_with_config(supervisor_config).await?;
                daemon.run().await?;
            }

            SupervisorCommands::Status => {
                // Use the async supervisor client to get status
                let client =
                    replicante::supervisor::async_client::AsyncSupervisorClient::new(None)?;
                match client.get_status().await {
                    Ok(status) => {
                        println!("Supervisor Status:");
                        println!("Total agents: {}", status.total_agents);
                        println!("Running agents: {}", status.running_agents);
                        if !status.agents.is_empty() {
                            println!("\nAgents:");
                            for agent in status.agents {
                                println!(
                                    "  - {} [{}] (started: {})",
                                    agent.id, agent.status, agent.started_at
                                );
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to get supervisor status: {}", e);
                        eprintln!("Is the supervisor running on http://localhost:8080?");
                    }
                }
            }

            SupervisorCommands::Stop { agent_id } => {
                let client =
                    replicante::supervisor::async_client::AsyncSupervisorClient::new(None)?;
                match client.stop_agent(&agent_id).await {
                    Ok(_) => println!("Successfully stopped agent: {}", agent_id),
                    Err(e) => eprintln!("Failed to stop agent: {}", e),
                }
            }

            SupervisorCommands::Kill { agent_id } => {
                let client =
                    replicante::supervisor::async_client::AsyncSupervisorClient::new(None)?;
                match client.kill_agent(&agent_id).await {
                    Ok(_) => println!("Successfully killed agent: {}", agent_id),
                    Err(e) => eprintln!("Failed to kill agent: {}", e),
                }
            }

            SupervisorCommands::Quarantine { agent_id } => {
                println!("Quarantining agent: {}", agent_id);
                println!("Note: Quarantine is currently implemented as pause in the supervisor");
                // We could add a pause endpoint if needed
            }

            SupervisorCommands::Logs { agent_id, follow } => {
                let client =
                    replicante::supervisor::async_client::AsyncSupervisorClient::new(None)?;
                if follow {
                    // Use streaming for follow mode
                    use futures::StreamExt;
                    match client.get_logs_stream(&agent_id, true, Some(100)).await {
                        Ok(stream) => {
                            println!("Following logs for agent {}...", agent_id);
                            futures::pin_mut!(stream);
                            while let Some(chunk) = stream.next().await {
                                match chunk {
                                    Ok(log) => print!("{}", log),
                                    Err(e) => {
                                        eprintln!("Stream error: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(e) => eprintln!("Failed to get logs stream: {}", e),
                    }
                } else {
                    match client.get_logs(&agent_id, false, Some(100)).await {
                        Ok(logs) => println!("{}", logs),
                        Err(e) => eprintln!("Failed to get logs: {}", e),
                    }
                }
            }
        },

        Commands::Sandbox { config, supervisor } => {
            info!("Starting agent in sandboxed environment");
            info!("Note: Sandboxing is enforced at Docker/infrastructure level");

            if let Some(supervisor_url) = supervisor {
                info!("Connecting to supervisor at: {}", supervisor_url);
            }

            run_sandboxed(config).await?;
        }

        Commands::Monitor { command } => {
            match command {
                MonitorCommands::Metrics { agent_id, format } => {
                    if let Some(id) = agent_id {
                        println!("Metrics for agent {}:", id);
                    } else {
                        println!("Metrics for all agents:");
                    }
                    println!("Format: {}", format);
                    println!("Not yet implemented - would fetch from supervisor");
                }

                MonitorCommands::Events { limit } => {
                    println!("Recent {} events:", limit);
                    println!("Not yet implemented - would fetch from supervisor");
                }

                MonitorCommands::Alerts { limit } => {
                    println!("Recent {} alerts:", limit);
                    println!("Not yet implemented - would fetch from supervisor");
                }

                MonitorCommands::Decisions { agent_id, last } => {
                    println!("Last {} decisions for agent {}:", last, agent_id);
                    println!("Not yet implemented - would fetch from supervisor");
                }

                MonitorCommands::Audit { export } => {
                    if let Some(path) = export {
                        println!("Exporting audit log to: {:?}", path);
                    } else {
                        println!("Audit log:");
                    }
                    println!("Not yet implemented - would fetch from supervisor");
                }

                MonitorCommands::Dashboard { url } => {
                    println!("Opening dashboard at: {}", url);
                    // Could use webbrowser crate to open in default browser
                    println!("Please open {} in your browser", url);
                }
            }
        }
    }

    Ok(())
}
