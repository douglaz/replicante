use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::process::Command;
use tokio::time::{Duration, interval};
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityScanner {
    scan_interval: Duration,
    container_whitelist: Vec<String>,
    process_whitelist: Vec<String>,
    syscall_whitelist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityReport {
    timestamp: chrono::DateTime<chrono::Utc>,
    container_id: String,
    findings: Vec<SecurityFinding>,
    risk_level: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecurityFinding {
    UnauthorizedProcess {
        pid: u32,
        name: String,
        cmdline: String,
    },
    SuspiciousSyscall {
        syscall: String,
        count: u32,
    },
    NetworkViolation {
        connection: String,
        port: u16,
    },
    FilesystemViolation {
        path: String,
        operation: String,
    },
    PrivilegeEscalation {
        details: String,
    },
    ResourceAnomaly {
        resource: String,
        value: f64,
        threshold: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl SecurityScanner {
    pub fn new() -> Self {
        Self {
            scan_interval: Duration::from_secs(30),
            container_whitelist: vec!["replicante".to_string(), "supervisor".to_string()],
            process_whitelist: vec![
                "replicante".to_string(),
                "supervisor".to_string(),
                "sh".to_string(),
                "bash".to_string(),
            ],
            syscall_whitelist: vec![
                "read".to_string(),
                "write".to_string(),
                "open".to_string(),
                "close".to_string(),
                "stat".to_string(),
                "mmap".to_string(),
                "munmap".to_string(),
                "brk".to_string(),
                "rt_sigaction".to_string(),
                "rt_sigprocmask".to_string(),
                "ioctl".to_string(),
                "access".to_string(),
                "select".to_string(),
                "poll".to_string(),
                "epoll_wait".to_string(),
            ],
        }
    }

    pub async fn start_scanning(&self) -> Result<()> {
        info!("Starting security scanner");

        let mut interval = interval(self.scan_interval);

        loop {
            interval.tick().await;

            if let Err(e) = self.scan_containers().await {
                error!("Security scan failed: {e}");
            }
        }
    }

    async fn scan_containers(&self) -> Result<()> {
        debug!("Running security scan");

        // Get list of running containers
        let containers = self.list_containers()?;

        for container_id in containers {
            if let Ok(report) = self.scan_container(&container_id).await
                && !report.findings.is_empty()
            {
                self.handle_security_report(report).await?;
            }
        }

        Ok(())
    }

    fn list_containers(&self) -> Result<Vec<String>> {
        let output = Command::new("docker")
            .args(["ps", "-q"])
            .output()
            .context("Failed to list docker containers")?;

        if !output.status.success() {
            bail!("Failed to list containers");
        }

        let container_ids = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect();

        Ok(container_ids)
    }

    async fn scan_container(&self, container_id: &str) -> Result<SecurityReport> {
        let mut findings = Vec::new();
        let mut max_risk = RiskLevel::Low;

        // Check running processes
        if let Ok(process_findings) = self.check_processes(container_id) {
            for finding in process_findings {
                if let SecurityFinding::UnauthorizedProcess { .. } = finding {
                    max_risk = self.escalate_risk(max_risk, RiskLevel::High);
                }
                findings.push(finding);
            }
        }

        // Check network connections
        if let Ok(network_findings) = self.check_network(container_id) {
            for finding in network_findings {
                if let SecurityFinding::NetworkViolation { .. } = finding {
                    max_risk = self.escalate_risk(max_risk, RiskLevel::Medium);
                }
                findings.push(finding);
            }
        }

        // Check filesystem access
        if let Ok(fs_findings) = self.check_filesystem(container_id) {
            for finding in fs_findings {
                if let SecurityFinding::FilesystemViolation { .. } = finding {
                    max_risk = self.escalate_risk(max_risk, RiskLevel::Medium);
                }
                findings.push(finding);
            }
        }

        // Check for privilege escalation attempts
        if let Ok(priv_findings) = self.check_privileges(container_id) {
            for finding in priv_findings {
                if let SecurityFinding::PrivilegeEscalation { .. } = finding {
                    max_risk = self.escalate_risk(max_risk, RiskLevel::Critical);
                }
                findings.push(finding);
            }
        }

        Ok(SecurityReport {
            timestamp: chrono::Utc::now(),
            container_id: container_id.to_string(),
            findings,
            risk_level: max_risk,
        })
    }

    fn check_processes(&self, container_id: &str) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();

        // Get process list from container
        let output = Command::new("docker")
            .args(["exec", container_id, "ps", "aux"])
            .output()
            .context("Failed to list processes in container")?;

        if output.status.success() {
            let processes = String::from_utf8_lossy(&output.stdout);

            for line in processes.lines().skip(1) {
                // Skip header
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() < 11 {
                    continue;
                }

                let process_name = parts[10];

                // Check if process is whitelisted
                if !self
                    .process_whitelist
                    .iter()
                    .any(|p| process_name.contains(p))
                {
                    findings.push(SecurityFinding::UnauthorizedProcess {
                        pid: parts[1].parse().unwrap_or(0),
                        name: process_name.to_string(),
                        cmdline: parts[10..].join(" "),
                    });
                }
            }
        }

        Ok(findings)
    }

    fn check_network(&self, container_id: &str) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();

        // Check network connections
        let output = Command::new("docker")
            .args(["exec", container_id, "ss", "-tuln"])
            .output()
            .context("Failed to check network connections")?;

        if output.status.success() {
            let connections = String::from_utf8_lossy(&output.stdout);

            for line in connections.lines().skip(1) {
                // Skip header
                if line.contains("LISTEN") || line.contains("ESTAB") {
                    // Parse connection details
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() < 5 {
                        continue;
                    }

                    let local_addr = parts[3];

                    // Check for suspicious ports
                    if let Some(port_str) = local_addr.split(':').next_back()
                        && let Ok(port) = port_str.parse::<u16>()
                        && self.is_suspicious_port(port)
                    {
                        findings.push(SecurityFinding::NetworkViolation {
                            connection: local_addr.to_string(),
                            port,
                        });
                    }
                }
            }
        }

        Ok(findings)
    }

    fn check_filesystem(&self, container_id: &str) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();

        // Check for suspicious file modifications
        let output = Command::new("docker")
            .args([
                "exec",
                container_id,
                "find",
                "/",
                "-type",
                "f",
                "-mmin",
                "-5",
                "-ls",
            ])
            .output()
            .context("Failed to check filesystem")?;

        if output.status.success() {
            let files = String::from_utf8_lossy(&output.stdout);

            for line in files.lines() {
                // Check for modifications to sensitive files
                if line.contains("/etc/passwd")
                    || line.contains("/etc/shadow")
                    || line.contains("/etc/sudoers")
                    || line.contains("/.ssh/")
                {
                    findings.push(SecurityFinding::FilesystemViolation {
                        path: line.to_string(),
                        operation: "modified".to_string(),
                    });
                }
            }
        }

        Ok(findings)
    }

    fn check_privileges(&self, container_id: &str) -> Result<Vec<SecurityFinding>> {
        let mut findings = Vec::new();

        // Check if container is running as root
        let output = Command::new("docker")
            .args(["exec", container_id, "id"])
            .output()
            .context("Failed to check user privileges")?;

        if output.status.success() {
            let id_output = String::from_utf8_lossy(&output.stdout);

            if id_output.contains("uid=0") {
                findings.push(SecurityFinding::PrivilegeEscalation {
                    details: "Container running as root".to_string(),
                });
            }
        }

        // Check for capability additions
        let output = Command::new("docker")
            .args([
                "inspect",
                container_id,
                "--format",
                "{{.HostConfig.CapAdd}}",
            ])
            .output()
            .context("Failed to check capabilities")?;

        if output.status.success() {
            let caps = String::from_utf8_lossy(&output.stdout);

            if caps.contains("SYS_ADMIN") || caps.contains("SYS_PTRACE") {
                findings.push(SecurityFinding::PrivilegeEscalation {
                    details: format!("Dangerous capabilities: {caps}"),
                });
            }
        }

        Ok(findings)
    }

    fn is_suspicious_port(&self, port: u16) -> bool {
        match port {
            22 | 23 | 135 | 139 | 445 | 3389 => true, // SSH, Telnet, SMB, RDP
            1337 | 31337 | 4444 | 6666 => true,       // Common backdoor ports
            _ => false,
        }
    }

    fn escalate_risk(&self, current: RiskLevel, new: RiskLevel) -> RiskLevel {
        match (current, new) {
            (RiskLevel::Critical, _) | (_, RiskLevel::Critical) => RiskLevel::Critical,
            (RiskLevel::High, _) | (_, RiskLevel::High) => RiskLevel::High,
            (RiskLevel::Medium, _) | (_, RiskLevel::Medium) => RiskLevel::Medium,
            _ => RiskLevel::Low,
        }
    }

    async fn handle_security_report(&self, report: SecurityReport) -> Result<()> {
        match report.risk_level {
            RiskLevel::Critical => {
                error!(
                    "CRITICAL SECURITY ISSUE in container {}: {:?}",
                    report.container_id, report.findings
                );
                // Could trigger emergency shutdown
            }
            RiskLevel::High => {
                warn!(
                    "High security risk in container {}: {:?}",
                    report.container_id, report.findings
                );
            }
            RiskLevel::Medium => {
                info!(
                    "Medium security risk in container {}: {:?}",
                    report.container_id, report.findings
                );
            }
            RiskLevel::Low => {
                debug!(
                    "Low security findings in container {}: {:?}",
                    report.container_id, report.findings
                );
            }
        }

        // Store report for audit
        self.store_security_report(report).await?;

        Ok(())
    }

    async fn store_security_report(&self, report: SecurityReport) -> Result<()> {
        // TODO: Store in database or send to supervisor
        let json = serde_json::to_string_pretty(&report)?;

        // For now, just log it
        debug!("Security report: {json}");

        Ok(())
    }
}

impl Default for SecurityScanner {
    fn default() -> Self {
        Self::new()
    }
}
