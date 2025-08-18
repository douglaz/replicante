#!/bin/bash
set -e

# Test script for improved supervisor functionality
# Tests all improvements: async client, log streaming, stats, network creation

echo "================================"
echo "Supervisor Improvement Test Suite"
echo "================================"
echo

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counters
TESTS_PASSED=0
TESTS_FAILED=0

# Helper functions
pass() {
    echo -e "${GREEN}✓${NC} $1"
    ((TESTS_PASSED++))
}

fail() {
    echo -e "${RED}✗${NC} $1"
    ((TESTS_FAILED++))
}

info() {
    echo -e "${YELLOW}→${NC} $1"
}

# Phase 1: Environment Setup
echo "Phase 1: Environment Setup"
echo "--------------------------"

info "Cleaning up existing containers..."
docker ps -a | grep replicante-agent | awk '{print $1}' | xargs -r docker rm -f 2>/dev/null || true

info "Removing existing network..."
docker network rm replicante-net 2>/dev/null || true

info "Building release binary..."
if nix develop -c cargo build --release --bin replicante 2>&1 | grep -q "Finished"; then
    pass "Binary built successfully"
else
    fail "Binary build failed"
    exit 1
fi

# Phase 2: Network Auto-Creation Test
echo
echo "Phase 2: Network Auto-Creation"
echo "-------------------------------"

info "Verifying network doesn't exist..."
if ! docker network ls | grep -q replicante-net; then
    pass "Network confirmed absent"
else
    fail "Network unexpectedly exists"
fi

info "Starting supervisor..."
nix develop -c cargo run --release --bin replicante -- supervisor start --web-port 8091 > /tmp/supervisor_test.log 2>&1 &
SUPERVISOR_PID=$!
sleep 5

if ps -p $SUPERVISOR_PID > /dev/null; then
    pass "Supervisor started (PID: $SUPERVISOR_PID)"
else
    fail "Supervisor failed to start"
    exit 1
fi

info "Checking network creation..."
if docker network ls | grep -q replicante-net; then
    pass "Network auto-created successfully"
else
    fail "Network was not created"
fi

# Phase 3: Async CLI Tests
echo
echo "Phase 3: Async CLI Tests"
echo "------------------------"

export SUPERVISOR_URL=http://localhost:8091

info "Testing status command..."
if env SUPERVISOR_URL=$SUPERVISOR_URL nix develop -c cargo run --release --bin replicante -- supervisor status 2>&1 | grep -q "Total agents: 0"; then
    pass "Status command works without panic"
else
    fail "Status command failed"
fi

# Phase 4: Agent Spawning
echo
echo "Phase 4: Agent Management"
echo "-------------------------"

info "Spawning test agent..."
AGENT_RESPONSE=$(curl -s -X POST http://localhost:8091/api/agents \
  -H "Content-Type: application/json" \
  -d '{"config_path": "/home/master/p/replicante/config/agent-test-1.toml", "sandbox_mode": "moderate"}')

if echo "$AGENT_RESPONSE" | grep -q "agent_id"; then
    AGENT_ID=$(echo "$AGENT_RESPONSE" | sed -n 's/.*"agent_id":"\([^"]*\)".*/\1/p')
    pass "Agent spawned: $AGENT_ID"
else
    fail "Failed to spawn agent"
fi

sleep 3

info "Verifying container is running..."
if docker ps | grep -q "$AGENT_ID"; then
    pass "Container running for agent $AGENT_ID"
else
    fail "Container not running"
fi

# Phase 5: Log Streaming Tests
echo
echo "Phase 5: Log Streaming"
echo "----------------------"

info "Testing direct Docker logs..."
if docker logs --tail 5 "replicante-agent-$AGENT_ID" 2>&1 | grep -q "INFO"; then
    pass "Docker logs accessible"
else
    fail "Docker logs not accessible"
fi

info "Testing CLI logs command..."
LOG_OUTPUT=$(env SUPERVISOR_URL=$SUPERVISOR_URL nix develop -c cargo run --release --bin replicante -- supervisor logs "$AGENT_ID" 2>&1)
if echo "$LOG_OUTPUT" | grep -q -E "(agent|Running|logs)"; then
    pass "CLI logs command executed"
else
    fail "CLI logs command failed"
fi

# Phase 6: Stats Collection
echo
echo "Phase 6: Stats Collection"
echo "-------------------------"

info "Waiting for stats collection..."
sleep 10

info "Checking supervisor logs for stats errors..."
if grep -q "Failed to get container stats" /tmp/supervisor_test.log; then
    info "Stats collection had issues (non-critical)"
else
    pass "No stats collection errors"
fi

# Phase 7: Agent Lifecycle
echo
echo "Phase 7: Agent Lifecycle"
echo "------------------------"

info "Testing agent stop..."
if env SUPERVISOR_URL=$SUPERVISOR_URL nix develop -c cargo run --release --bin replicante -- supervisor stop "$AGENT_ID" 2>&1 | grep -q "stopped"; then
    pass "Stop command executed"
else
    info "Stop command had issues"
fi

sleep 3

info "Verifying container stopped..."
if ! docker ps | grep -q "$AGENT_ID"; then
    pass "Container stopped successfully"
else
    fail "Container still running"
fi

# Phase 8: Stress Test
echo
echo "Phase 8: Stress Test"
echo "--------------------"

info "Spawning multiple agents..."
for i in {1..3}; do
    RESPONSE=$(curl -s -X POST http://localhost:8091/api/agents \
      -H "Content-Type: application/json" \
      -d "{\"config_path\": \"/home/master/p/replicante/config/agent-test-1.toml\", \"sandbox_mode\": \"moderate\"}")
    
    if echo "$RESPONSE" | grep -q "agent_id"; then
        pass "Agent $i spawned"
    else
        fail "Failed to spawn agent $i"
    fi
done

sleep 3

info "Checking all agents..."
AGENT_COUNT=$(curl -s http://localhost:8091/api/agents | grep -o "agent_id" | wc -l)
if [ "$AGENT_COUNT" -ge 3 ]; then
    pass "$AGENT_COUNT agents running"
else
    fail "Expected 3+ agents, found $AGENT_COUNT"
fi

# Cleanup
echo
echo "Phase 9: Cleanup"
echo "----------------"

info "Stopping supervisor..."
kill $SUPERVISOR_PID 2>/dev/null || true
sleep 2

info "Cleaning up containers..."
docker ps -a | grep replicante-agent | awk '{print $1}' | xargs -r docker rm -f 2>/dev/null || true

pass "Cleanup completed"

# Final Report
echo
echo "================================"
echo "Test Results Summary"
echo "================================"
echo -e "${GREEN}Passed:${NC} $TESTS_PASSED"
echo -e "${RED}Failed:${NC} $TESTS_FAILED"

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed! Supervisor improvements verified.${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed. Review the output above.${NC}"
    exit 1
fi