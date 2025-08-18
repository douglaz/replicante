#!/bin/bash
# Local Supervisor Testing Script
# Tests supervisor without Docker requirements

set -e

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

function log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

function log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

function cleanup() {
    log_info "Cleaning up..."
    pkill -f "replicante supervisor" 2>/dev/null || true
}

trap cleanup EXIT

log_info "Starting Local Supervisor Test"

# Build the binary
log_info "Building replicante..."
nix develop -c cargo build --bin replicante

# Start supervisor in background
log_info "Starting supervisor daemon..."
RUST_LOG=info nix develop -c cargo run --bin replicante -- supervisor start &
SUPERVISOR_PID=$!

# Wait for startup
log_info "Waiting for supervisor to start..."
sleep 3

# Check if supervisor is running
if ! kill -0 $SUPERVISOR_PID 2>/dev/null; then
    log_error "Supervisor failed to start"
    exit 1
fi

# Test API status
log_info "Testing API status endpoint..."
if curl -f -s http://localhost:8080/api/status > /dev/null; then
    log_info "✓ API is responding"
    curl -s http://localhost:8080/api/status | jq .
else
    log_error "API not responding"
    exit 1
fi

# Test CLI status command
log_info "Testing CLI status command..."
if nix develop -c cargo run --bin replicante -- supervisor status; then
    log_info "✓ CLI status command works"
else
    log_error "CLI status command failed"
fi

# Test dashboard
log_info "Testing dashboard..."
if curl -s http://localhost:8080/ | grep -q "Replicante Supervisor"; then
    log_info "✓ Dashboard is accessible"
else
    log_error "Dashboard not found"
fi

log_info "Test completed successfully!"
log_info "Supervisor PID: $SUPERVISOR_PID"
log_info "Press Ctrl+C to stop..."

# Keep running for manual testing
wait $SUPERVISOR_PID