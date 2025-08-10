#!/bin/bash
# Monitor network traffic and alert on suspicious patterns
# This script analyzes network connections and reports anomalies

set -e

LOG_FILE="/logs/network/monitor.log"
ALERT_FILE="/logs/network/alerts.log"
CHECK_INTERVAL=5

# Whitelist of allowed connections
declare -A ALLOWED_PORTS=(
    [80]=1
    [443]=1
    [53]=1
    [3128]=1
    [8080]=1
)

declare -A ALLOWED_IPS=(
    ["127.0.0.1"]=1
    ["172.20.0.2"]=1  # Proxy
    ["172.20.0.3"]=1  # DNS
    ["172.20.0.5"]=1  # Supervisor
)

log_message() {
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] $1" >> "$LOG_FILE"
}

alert() {
    local message="$1"
    echo "[$(date '+%Y-%m-%d %H:%M:%S')] ALERT: $message" | tee -a "$ALERT_FILE"
    # Could also send to supervisor API here
}

check_connections() {
    # Get current network connections
    local connections=$(ss -tunap 2>/dev/null | grep ESTAB)
    
    while IFS= read -r line; do
        # Parse connection details
        local proto=$(echo "$line" | awk '{print $1}')
        local local_addr=$(echo "$line" | awk '{print $4}')
        local remote_addr=$(echo "$line" | awk '{print $5}')
        
        # Extract IP and port from remote address
        local remote_ip=$(echo "$remote_addr" | cut -d: -f1)
        local remote_port=$(echo "$remote_addr" | cut -d: -f2)
        
        # Check if connection is allowed
        if [[ -z "${ALLOWED_IPS[$remote_ip]}" ]] && [[ ! "$remote_ip" =~ ^172\.20\. ]]; then
            if [[ -z "${ALLOWED_PORTS[$remote_port]}" ]]; then
                alert "Suspicious connection: $proto to $remote_addr"
            fi
        fi
    done <<< "$connections"
}

check_dns_queries() {
    # Monitor DNS queries if dnsmasq log is available
    local dns_log="/var/log/dnsmasq.log"
    if [[ -f "$dns_log" ]]; then
        # Check for unusual DNS queries
        tail -n 100 "$dns_log" | while read -r line; do
            if echo "$line" | grep -qE "query\[A\].*\.(tk|ml|ga|cf)$"; then
                alert "Suspicious DNS query: $line"
            fi
        done
    fi
}

check_bandwidth() {
    # Monitor bandwidth usage
    local rx_bytes_before=$(cat /sys/class/net/eth0/statistics/rx_bytes 2>/dev/null || echo 0)
    local tx_bytes_before=$(cat /sys/class/net/eth0/statistics/tx_bytes 2>/dev/null || echo 0)
    
    sleep 1
    
    local rx_bytes_after=$(cat /sys/class/net/eth0/statistics/rx_bytes 2>/dev/null || echo 0)
    local tx_bytes_after=$(cat /sys/class/net/eth0/statistics/tx_bytes 2>/dev/null || echo 0)
    
    local rx_rate=$((rx_bytes_after - rx_bytes_before))
    local tx_rate=$((tx_bytes_after - tx_bytes_before))
    
    # Alert if bandwidth exceeds 10MB/s
    if [[ $rx_rate -gt 10485760 ]] || [[ $tx_rate -gt 10485760 ]]; then
        alert "High bandwidth usage: RX=$((rx_rate/1024/1024))MB/s TX=$((tx_rate/1024/1024))MB/s"
    fi
}

check_failed_connections() {
    # Check for failed connection attempts in kernel log
    dmesg | tail -n 50 | grep -i "iptables-dropped" | while read -r line; do
        log_message "Blocked connection: $line"
    done
}

# Main monitoring loop
main() {
    log_message "Network monitor started"
    
    # Create log directories
    mkdir -p "$(dirname "$LOG_FILE")"
    mkdir -p "$(dirname "$ALERT_FILE")"
    
    while true; do
        check_connections
        check_dns_queries
        check_bandwidth
        check_failed_connections
        
        sleep "$CHECK_INTERVAL"
    done
}

# Trap signals for clean shutdown
trap 'log_message "Monitor shutting down"; exit 0' SIGTERM SIGINT

main