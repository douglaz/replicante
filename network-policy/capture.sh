#!/bin/bash
# Network traffic capture script for monitoring
# Runs tcpdump with filters to capture suspicious traffic

set -e

PCAP_DIR="/pcap"
INTERFACE="eth0"
ROTATE_SIZE="10M"
ROTATE_TIME="3600"  # 1 hour
MAX_FILES="24"  # Keep 24 hours of logs

echo "Starting network traffic capture on $INTERFACE..."

# Create output directory if it doesn't exist
mkdir -p $PCAP_DIR

# Capture filter - focus on non-local traffic
FILTER="not host 127.0.0.1 and not net 172.20.0.0/16 and (tcp or udp)"

# Run tcpdump with rotation
tcpdump -i $INTERFACE \
    -w "$PCAP_DIR/capture_%Y%m%d_%H%M%S.pcap" \
    -W $MAX_FILES \
    -C $ROTATE_SIZE \
    -G $ROTATE_TIME \
    -Z root \
    "$FILTER" \
    -v

# Note: This will run indefinitely
# The container should be configured with restart policy