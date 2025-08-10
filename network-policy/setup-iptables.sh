#!/bin/bash
# Setup iptables rules for network filtering in container
# This provides kernel-level network security

set -e

echo "Setting up iptables network filtering rules..."

# Flush existing rules
iptables -F
iptables -X
iptables -t nat -F
iptables -t nat -X
iptables -t mangle -F
iptables -t mangle -X

# Set default policies - deny all
iptables -P INPUT DROP
iptables -P FORWARD DROP
iptables -P OUTPUT DROP

# Allow loopback
iptables -A INPUT -i lo -j ACCEPT
iptables -A OUTPUT -o lo -j ACCEPT

# Allow established connections
iptables -A INPUT -m state --state ESTABLISHED,RELATED -j ACCEPT
iptables -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT

# Allow DNS (port 53) to DNS server only
DNS_SERVER="172.20.0.3"
iptables -A OUTPUT -p udp --dport 53 -d $DNS_SERVER -j ACCEPT
iptables -A OUTPUT -p tcp --dport 53 -d $DNS_SERVER -j ACCEPT

# Allow HTTP/HTTPS to proxy only
PROXY_SERVER="172.20.0.2"
iptables -A OUTPUT -p tcp --dport 3128 -d $PROXY_SERVER -j ACCEPT

# Allow communication with supervisor
SUPERVISOR="172.20.0.5"
iptables -A OUTPUT -p tcp --dport 8080 -d $SUPERVISOR -j ACCEPT

# Allow specific ports for whitelisted IPs
# Anthropic API
ANTHROPIC_IPS="142.251.1.0/24"  # Example - replace with actual IPs
iptables -A OUTPUT -p tcp --dport 443 -d $ANTHROPIC_IPS -j ACCEPT

# OpenAI API  
OPENAI_IPS="104.18.0.0/16"  # Example - replace with actual IPs
iptables -A OUTPUT -p tcp --dport 443 -d $OPENAI_IPS -j ACCEPT

# Log dropped packets for debugging
iptables -A INPUT -j LOG --log-prefix "iptables-dropped-input: " --log-level 4
iptables -A OUTPUT -j LOG --log-prefix "iptables-dropped-output: " --log-level 4

# Rate limiting to prevent flooding
iptables -A OUTPUT -m limit --limit 100/minute --limit-burst 200 -j ACCEPT

# Connection limiting
iptables -A OUTPUT -p tcp --syn -m connlimit --connlimit-above 10 --connlimit-mask 32 -j REJECT

# Prevent port scanning
iptables -N port-scanning
iptables -A port-scanning -p tcp --tcp-flags SYN,ACK,FIN,RST RST -m limit --limit 1/s --limit-burst 2 -j RETURN
iptables -A port-scanning -j DROP

# Save rules (if iptables-persistent is installed)
if command -v iptables-save >/dev/null 2>&1; then
    iptables-save > /etc/iptables/rules.v4 2>/dev/null || true
fi

# Display current rules
echo "Current iptables rules:"
iptables -L -v -n

echo "iptables setup complete"