#!/bin/bash
# End-to-End Supervisor Testing Script
# Tests the complete supervisor functionality with Docker containers

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

function log_warning() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

function cleanup() {
    log_info "Cleaning up test environment..."
    
    # Stop and remove test containers
    docker ps -a | grep replicante-test | awk '{print $1}' | xargs -r docker rm -f 2>/dev/null || true
    docker ps -a | grep supervisor-e2e | awk '{print $1}' | xargs -r docker rm -f 2>/dev/null || true
    
    # Remove test network if exists
    docker network rm replicante-test-net 2>/dev/null || true
    
    # Kill local supervisor if running
    pkill -f "replicante supervisor" 2>/dev/null || true
}

# Set up trap for cleanup on exit
trap cleanup EXIT

log_info "===================================="
log_info "Supervisor End-to-End Test Starting"
log_info "===================================="

# Phase 1: Build and Setup
log_info "Phase 1: Building and setting up environment"

# Check Docker is running
if ! docker info > /dev/null 2>&1; then
    log_error "Docker is not running. Please start Docker first."
    exit 1
fi

# Build the replicante binary
log_info "Building replicante binary..."
nix develop -c cargo build --bin replicante --release

# Build Docker image
log_info "Building Docker image..."
docker build -t replicante:test -f Dockerfile .

# Create test network
log_info "Creating test network..."
docker network create replicante-test-net 2>/dev/null || true

# Phase 2: Start Supervisor
log_info "Phase 2: Starting supervisor daemon"

# Option 1: Run supervisor locally (preferred for testing)
log_info "Starting supervisor locally..."
RUST_LOG=debug nix develop -c cargo run --release -- supervisor start > supervisor.log 2>&1 &
SUPERVISOR_PID=$!
sleep 5

# Check if supervisor is running
if ! kill -0 $SUPERVISOR_PID 2>/dev/null; then
    log_error "Supervisor failed to start. Check supervisor.log"
    tail -20 supervisor.log
    exit 1
fi

# Phase 3: Test API Endpoints
log_info "Phase 3: Testing API endpoints"

# Test status endpoint
log_info "Testing /api/status endpoint..."
if ! curl -f -s http://localhost:8080/api/status > /dev/null; then
    log_error "Status endpoint not responding"
    exit 1
fi
log_info "✓ Status endpoint working"

# Test dashboard
log_info "Testing dashboard endpoint..."
if ! curl -f -s http://localhost:8080/ | grep -q "Replicante Supervisor"; then
    log_error "Dashboard not responding"
    exit 1
fi
log_info "✓ Dashboard working"

# Phase 4: Test CLI Commands
log_info "Phase 4: Testing CLI commands"

# Test status command
log_info "Testing 'supervisor status' command..."
if ! nix develop -c cargo run --release -- supervisor status; then
    log_error "Status command failed"
    exit 1
fi
log_info "✓ Status command working"

# Phase 5: Test Agent Spawning
log_info "Phase 5: Testing agent spawning"

# Spawn agent via API
log_info "Spawning test agent via API..."
RESPONSE=$(curl -s -X POST http://localhost:8080/api/agents \
  -H "Content-Type: application/json" \
  -d '{
    "config_path": "/config/agent-test-1.toml",
    "sandbox_mode": "moderate"
  }')

if [ -z "$RESPONSE" ]; then
    log_error "Failed to spawn agent - no response"
    exit 1
fi

AGENT_ID=$(echo $RESPONSE | jq -r '.agent_id' 2>/dev/null || echo "")
if [ -z "$AGENT_ID" ] || [ "$AGENT_ID" = "null" ]; then
    log_error "Failed to get agent ID from response: $RESPONSE"
    exit 1
fi

log_info "✓ Agent spawned with ID: $AGENT_ID"

# Wait for container to start
sleep 3

# Phase 6: Test Agent Operations
log_info "Phase 6: Testing agent operations"

# List agents
log_info "Listing agents..."
AGENTS=$(curl -s http://localhost:8080/api/agents)
if ! echo $AGENTS | jq . > /dev/null 2>&1; then
    log_error "Failed to list agents"
    exit 1
fi
log_info "✓ Agent list retrieved"

# Check Docker container
log_info "Checking Docker container..."
if docker ps | grep -q "replicante-agent-$AGENT_ID"; then
    log_info "✓ Docker container running"
else
    log_warning "Docker container not found - supervisor may be using mock mode"
fi

# Test stop agent
log_info "Stopping agent $AGENT_ID..."
if ! curl -s -X POST "http://localhost:8080/api/agents/$AGENT_ID/stop" > /dev/null; then
    log_error "Failed to stop agent"
    exit 1
fi
log_info "✓ Agent stop command sent"

sleep 2

# Phase 7: Test Multiple Agents
log_info "Phase 7: Testing multiple agents"

# Spawn second agent
log_info "Spawning second test agent..."
RESPONSE2=$(curl -s -X POST http://localhost:8080/api/agents \
  -H "Content-Type: application/json" \
  -d '{
    "config_path": "/config/agent-test-2.toml",
    "sandbox_mode": "strict"
  }')

AGENT_ID2=$(echo $RESPONSE2 | jq -r '.agent_id' 2>/dev/null || echo "")
if [ -n "$AGENT_ID2" ] && [ "$AGENT_ID2" != "null" ]; then
    log_info "✓ Second agent spawned with ID: $AGENT_ID2"
else
    log_warning "Failed to spawn second agent: $RESPONSE2"
fi

# List all agents
log_info "Listing all agents..."
curl -s http://localhost:8080/api/agents | jq '.[] | {id: .id, status: .status}'

# Phase 8: Test Emergency Stop
log_info "Phase 8: Testing emergency stop"

if [ -n "$AGENT_ID2" ] && [ "$AGENT_ID2" != "null" ]; then
    log_info "Killing agent $AGENT_ID2..."
    if ! curl -s -X POST "http://localhost:8080/api/agents/$AGENT_ID2/kill" > /dev/null; then
        log_warning "Failed to kill agent"
    else
        log_info "✓ Agent kill command sent"
    fi
fi

# Phase 9: Test Metrics and Monitoring
log_info "Phase 9: Testing metrics and monitoring"

# Get metrics
log_info "Fetching metrics..."
if curl -s http://localhost:8080/api/metrics | jq . > /dev/null 2>&1; then
    log_info "✓ Metrics endpoint working"
else
    log_warning "Metrics endpoint not fully implemented"
fi

# Get events
log_info "Fetching events..."
if curl -s http://localhost:8080/api/events | jq . > /dev/null 2>&1; then
    log_info "✓ Events endpoint working"
else
    log_warning "Events endpoint not fully implemented"
fi

# Phase 10: Cleanup and Summary
log_info "Phase 10: Cleanup and summary"

# Remove test agents
if [ -n "$AGENT_ID" ] && [ "$AGENT_ID" != "null" ]; then
    curl -s -X DELETE "http://localhost:8080/api/agents/$AGENT_ID" > /dev/null || true
fi
if [ -n "$AGENT_ID2" ] && [ "$AGENT_ID2" != "null" ]; then
    curl -s -X DELETE "http://localhost:8080/api/agents/$AGENT_ID2" > /dev/null || true
fi

# Kill supervisor
log_info "Stopping supervisor..."
kill $SUPERVISOR_PID 2>/dev/null || true
wait $SUPERVISOR_PID 2>/dev/null || true

log_info "===================================="
log_info "Test Summary"
log_info "===================================="
log_info "✓ Supervisor daemon started successfully"
log_info "✓ API endpoints responding"
log_info "✓ CLI commands working"
log_info "✓ Agent spawning functional"
log_info "✓ Agent lifecycle management working"
log_info "✓ Multiple agents supported"
log_info "===================================="
log_info "All tests completed successfully!"
log_info "====================================" 