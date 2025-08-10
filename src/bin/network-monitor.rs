use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetworkStats {
    timestamp: chrono::DateTime<chrono::Utc>,
    connections: Vec<Connection>,
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
    active_connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Connection {
    protocol: String,
    local_addr: String,
    remote_addr: String,
    state: String,
    pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetworkAlert {
    timestamp: chrono::DateTime<chrono::Utc>,
    alert_type: AlertType,
    details: String,
    connection: Option<Connection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum AlertType {
    UnauthorizedConnection,
    HighBandwidth,
    SuspiciousPort,
    TooManyConnections,
    DNSAnomaly,
}

struct NetworkMonitor {
    supervisor_url: Option<String>,
    monitor_interval: Duration,
    alerts: Vec<NetworkAlert>,
    previous_stats: Option<NetworkStats>,
    whitelist: HashMap<String, bool>,
}

impl NetworkMonitor {
    fn new() -> Self {
        let supervisor_url = std::env::var("SUPERVISOR_URL").ok();
        let interval_secs = std::env::var("MONITOR_INTERVAL")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(5);

        let mut whitelist = HashMap::new();
        // Allowed IPs/networks
        whitelist.insert("127.0.0.1".to_string(), true);
        whitelist.insert("172.20.".to_string(), true); // Docker network prefix

        Self {
            supervisor_url,
            monitor_interval: Duration::from_secs(interval_secs),
            alerts: Vec::new(),
            previous_stats: None,
            whitelist,
        }
    }

    async fn run(&mut self) -> Result<()> {
        info!("Starting network monitor");
        info!("Monitor interval: {:?}", self.monitor_interval);

        if let Some(ref url) = self.supervisor_url {
            info!("Reporting to supervisor at: {}", url);
        }

        let mut interval = interval(self.monitor_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.monitor_cycle().await {
                error!("Monitor cycle failed: {}", e);
            }
        }
    }

    async fn monitor_cycle(&mut self) -> Result<()> {
        debug!("Starting monitor cycle");

        // Collect network statistics
        let stats = self.collect_network_stats()?;

        // Analyze for anomalies
        self.analyze_connections(&stats)?;

        // Check bandwidth usage
        if let Some(prev) = self.previous_stats.clone() {
            self.check_bandwidth(&prev, &stats)?;
        }

        // Report to supervisor if configured
        if self.supervisor_url.is_some() {
            self.report_to_supervisor(&stats).await?;
        }

        // Store current stats for next comparison
        self.previous_stats = Some(stats);

        Ok(())
    }

    fn collect_network_stats(&self) -> Result<NetworkStats> {
        let mut stats = NetworkStats {
            timestamp: chrono::Utc::now(),
            connections: Vec::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            active_connections: 0,
        };

        // Read TCP connections from /proc/net/tcp
        if let Ok(connections) = self.read_proc_net_tcp("/host/proc/net/tcp") {
            stats.connections.extend(connections);
        }

        // Read TCP6 connections
        if let Ok(connections) = self.read_proc_net_tcp("/host/proc/net/tcp6") {
            stats.connections.extend(connections);
        }

        // Read network interface stats from /proc/net/dev
        if let Ok((rx, tx, rx_p, tx_p)) = self.read_proc_net_dev("/host/proc/net/dev") {
            stats.rx_bytes = rx;
            stats.tx_bytes = tx;
            stats.rx_packets = rx_p;
            stats.tx_packets = tx_p;
        }

        stats.active_connections = stats.connections.len();

        debug!("Collected {} connections", stats.active_connections);

        Ok(stats)
    }

    fn read_proc_net_tcp(&self, path: &str) -> Result<Vec<Connection>> {
        let file = File::open(path).with_context(|| format!("Failed to open {}", path))?;
        let reader = BufReader::new(file);
        let mut connections = Vec::new();

        for (index, line) in reader.lines().enumerate() {
            if index == 0 {
                continue;
            } // Skip header

            let line = line?;
            let parts: Vec<&str> = line.split_whitespace().collect();

            if parts.len() < 10 {
                continue;
            }

            let local_addr = self.parse_hex_addr(parts[1])?;
            let remote_addr = self.parse_hex_addr(parts[2])?;
            let state = self.parse_tcp_state(parts[3])?;

            connections.push(Connection {
                protocol: "TCP".to_string(),
                local_addr,
                remote_addr,
                state,
                pid: None, // Would need to read from /proc/*/fd/* to get PID
            });
        }

        Ok(connections)
    }

    fn parse_hex_addr(&self, hex_addr: &str) -> Result<String> {
        let parts: Vec<&str> = hex_addr.split(':').collect();
        if parts.len() != 2 {
            return Ok("unknown".to_string());
        }

        let ip = u32::from_str_radix(parts[0], 16).unwrap_or(0);
        let port = u16::from_str_radix(parts[1], 16).unwrap_or(0);

        let ip_str = format!(
            "{}.{}.{}.{}",
            ip & 0xFF,
            (ip >> 8) & 0xFF,
            (ip >> 16) & 0xFF,
            (ip >> 24) & 0xFF
        );

        Ok(format!("{}:{}", ip_str, port))
    }

    fn parse_tcp_state(&self, hex_state: &str) -> Result<String> {
        let state_num = u8::from_str_radix(hex_state, 16).unwrap_or(0);

        let state = match state_num {
            1 => "ESTABLISHED",
            2 => "SYN_SENT",
            3 => "SYN_RECV",
            4 => "FIN_WAIT1",
            5 => "FIN_WAIT2",
            6 => "TIME_WAIT",
            7 => "CLOSE",
            8 => "CLOSE_WAIT",
            9 => "LAST_ACK",
            10 => "LISTEN",
            11 => "CLOSING",
            _ => "UNKNOWN",
        };

        Ok(state.to_string())
    }

    fn read_proc_net_dev(&self, path: &str) -> Result<(u64, u64, u64, u64)> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut total_rx_bytes = 0u64;
        let mut total_tx_bytes = 0u64;
        let mut total_rx_packets = 0u64;
        let mut total_tx_packets = 0u64;

        for line in reader.lines().skip(2) {
            // Skip headers
            let line = line?;
            let parts: Vec<&str> = line.split_whitespace().collect();

            if parts.len() >= 10 {
                // Skip loopback
                if parts[0].starts_with("lo:") {
                    continue;
                }

                total_rx_bytes += parts[1].parse::<u64>().unwrap_or(0);
                total_rx_packets += parts[2].parse::<u64>().unwrap_or(0);
                total_tx_bytes += parts[9].parse::<u64>().unwrap_or(0);
                total_tx_packets += parts[10].parse::<u64>().unwrap_or(0);
            }
        }

        Ok((
            total_rx_bytes,
            total_tx_bytes,
            total_rx_packets,
            total_tx_packets,
        ))
    }

    fn analyze_connections(&mut self, stats: &NetworkStats) -> Result<()> {
        for conn in &stats.connections {
            // Skip localhost and listening connections
            if conn.remote_addr.starts_with("0.0.0.0")
                || conn.remote_addr.starts_with("127.0.0.1")
                || conn.state == "LISTEN"
            {
                continue;
            }

            // Check if connection is to allowed IP
            let remote_ip = conn.remote_addr.split(':').next().unwrap_or("");
            let mut allowed = false;

            for prefix in self.whitelist.keys() {
                if remote_ip.starts_with(prefix) {
                    allowed = true;
                    break;
                }
            }

            if !allowed && conn.state == "ESTABLISHED" {
                warn!("Unauthorized connection detected: {}", conn.remote_addr);

                self.alerts.push(NetworkAlert {
                    timestamp: chrono::Utc::now(),
                    alert_type: AlertType::UnauthorizedConnection,
                    details: format!("Unauthorized connection to {}", conn.remote_addr),
                    connection: Some(conn.clone()),
                });
            }

            // Check for suspicious ports
            if let Some(port_str) = conn.remote_addr.split(':').nth(1)
                && let Ok(port) = port_str.parse::<u16>()
                && self.is_suspicious_port(port)
            {
                warn!("Connection to suspicious port: {}", port);

                self.alerts.push(NetworkAlert {
                    timestamp: chrono::Utc::now(),
                    alert_type: AlertType::SuspiciousPort,
                    details: format!("Connection to suspicious port {}", port),
                    connection: Some(conn.clone()),
                });
            }
        }

        // Check for too many connections
        if stats.active_connections > 100 {
            warn!("High number of connections: {}", stats.active_connections);

            self.alerts.push(NetworkAlert {
                timestamp: chrono::Utc::now(),
                alert_type: AlertType::TooManyConnections,
                details: format!("{} active connections", stats.active_connections),
                connection: None,
            });
        }

        Ok(())
    }

    fn is_suspicious_port(&self, port: u16) -> bool {
        match port {
            22 | 23 | 135 | 139 | 445 | 3389 | 5900 => true, // SSH, Telnet, SMB, RDP, VNC
            1337 | 31337 | 4444 | 6666 | 6667 => true,       // Common backdoor ports
            _ => false,
        }
    }

    fn check_bandwidth(&mut self, prev: &NetworkStats, curr: &NetworkStats) -> Result<()> {
        let time_diff = (curr.timestamp - prev.timestamp).num_seconds() as f64;
        if time_diff <= 0.0 {
            return Ok(());
        }

        let rx_rate = ((curr.rx_bytes - prev.rx_bytes) as f64 / time_diff) / 1024.0 / 1024.0; // MB/s
        let tx_rate = ((curr.tx_bytes - prev.tx_bytes) as f64 / time_diff) / 1024.0 / 1024.0; // MB/s

        debug!("Bandwidth: RX={:.2} MB/s, TX={:.2} MB/s", rx_rate, tx_rate);

        // Alert on high bandwidth (> 10 MB/s)
        if rx_rate > 10.0 || tx_rate > 10.0 {
            warn!(
                "High bandwidth usage: RX={:.2} MB/s, TX={:.2} MB/s",
                rx_rate, tx_rate
            );

            self.alerts.push(NetworkAlert {
                timestamp: chrono::Utc::now(),
                alert_type: AlertType::HighBandwidth,
                details: format!(
                    "High bandwidth: RX={:.2} MB/s, TX={:.2} MB/s",
                    rx_rate, tx_rate
                ),
                connection: None,
            });
        }

        Ok(())
    }

    async fn report_to_supervisor(&self, stats: &NetworkStats) -> Result<()> {
        if let Some(ref _url) = self.supervisor_url {
            // TODO: Send stats to supervisor API
            debug!(
                "Would report to supervisor: {} connections",
                stats.active_connections
            );
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Network Monitor starting");

    let mut monitor = NetworkMonitor::new();
    monitor.run().await?;

    Ok(())
}
