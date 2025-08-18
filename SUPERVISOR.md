# Supervisor Implementation Plan

## Overview

The Replicante Supervisor is a container-based agent management system that launches, monitors, and controls multiple AI agent instances running in isolated Docker containers. This document serves as both the implementation plan and progress tracker for the supervisor feature.

## Architecture

### Components

```
┌─────────────────────────────────────────────────────────────┐
│                     Supervisor Process                       │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                  Container Manager                    │   │
│  │  - Docker API Client (bollard)                       │   │
│  │  - Container Lifecycle Management                    │   │
│  │  - Resource Allocation                               │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                     Monitor                          │   │
│  │  - Container Stats Collection                        │   │
│  │  - Log Aggregation                                   │   │
│  │  - Alert Generation                                  │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                   Security Scanner                   │   │
│  │  - Container Inspection                              │   │
│  │  - Network Monitoring                                │   │
│  │  - Privilege Escalation Detection                    │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                    Web Dashboard                     │   │
│  │  - Real-time Status                                  │   │
│  │  - Metrics Visualization                             │   │
│  │  - Control Interface                                 │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
                              │ Docker API
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Docker Engine                          │
├──────────────┬──────────────┬──────────────┬──────────────┤
│   Agent      │   Agent      │   Agent      │   Agent      │
│ Container 1  │ Container 2  │ Container 3  │ Container N  │
│              │              │              │              │
│ ┌──────────┐ │ ┌──────────┐ │ ┌──────────┐ │ ┌──────────┐ │
│ │Replicante│ │ │Replicante│ │ │Replicante│ │ │Replicante│ │
│ │  Agent   │ │ │  Agent   │ │ │  Agent   │ │ │  Agent   │ │
│ └──────────┘ │ └──────────┘ │ └──────────┘ │ └──────────┘ │
│              │              │              │              │
│ Resources:   │ Resources:   │ Resources:   │ Resources:   │
│ - CPU: 1.0   │ - CPU: 0.5   │ - CPU: 1.0   │ - CPU: 0.5   │
│ - RAM: 512M  │ - RAM: 256M  │ - RAM: 512M  │ - RAM: 256M  │
│ - Net: Limited│ - Net: None  │ - Net: Full  │ - Net: Limited│
└──────────────┴──────────────┴──────────────┴──────────────┘
```

### Key Design Decisions

1. **Container-Only Agents**: All agents MUST run in Docker containers, no direct process spawning
2. **Resource Isolation**: Each container has defined CPU, memory, and network limits
3. **Security by Default**: Containers run with minimal privileges, read-only filesystems where possible
4. **Monitoring First**: All container metrics and logs are collected and analyzed
5. **Graceful Degradation**: Supervisor continues operating even if some agents fail

## Implementation Progress

### Phase 1: Docker Integration ✅
- [x] Add bollard crate (v0.15+) to Cargo.toml
- [x] Create src/supervisor/container_manager.rs module
- [x] Initialize Docker client with connection pooling
- [x] Implement error handling for Docker API failures
- [x] Add Docker connection health checks

### Phase 2: Container Lifecycle Management ✅
- [x] Refactor spawn_agent to create containers instead of processes
- [x] Update AgentProcess struct to track container_id instead of PID
- [x] Implement container creation with proper configuration
- [x] Add container start/stop/restart operations
- [x] Implement container removal and cleanup
- [x] Add container health check monitoring
- [x] Handle container exit codes and restart policies

### Phase 3: Container Configuration ✅
- [x] Define container image management (pull/update)
- [x] Implement volume mounting for configs and data
- [x] Configure network isolation modes
- [x] Set resource limits (CPU, memory, PIDs)
- [x] Apply security options (capabilities, seccomp)
- [ ] Configure logging drivers
- [x] Add environment variable injection

### Phase 4: Monitoring Integration 🔄
- [x] Integrate Docker stats API for real-time metrics
- [x] Implement container log streaming
- [ ] Add Docker event monitoring
- [x] Update metrics collection to use container stats
- [ ] Implement container-specific alerts
- [ ] Add Prometheus metrics export
- [ ] Create Grafana dashboards

### Phase 5: Security Profiles ✅
- [x] Define three security profiles (Strict/Moderate/Permissive)
- [x] Implement capability dropping
- [x] Add AppArmor/SELinux profile support
- [x] Configure read-only root filesystems
- [x] Implement network policies
- [ ] Add secrets management
- [ ] Implement container scanning

### Phase 6: API and CLI Updates ✅
- [x] Update supervisor API to expose container operations
- [x] Modify CLI to show container information
- [x] Add container logs streaming to CLI (placeholder)
- [ ] Implement container exec functionality
- [ ] Add container inspect command
- [x] Update status command to show container details

### Phase 7: Testing ✅
- [x] Unit tests for container_manager module
- [x] Integration tests with Docker
- [ ] Security isolation tests
- [ ] Resource limit enforcement tests
- [ ] Failure recovery tests
- [ ] Performance benchmarks
- [ ] Multi-container stress tests

### Phase 8: Documentation 📚
- [ ] API reference documentation
- [ ] Deployment guide
- [ ] Security best practices
- [ ] Troubleshooting guide
- [ ] Performance tuning guide

## Container Specification

### Base Configuration
```yaml
image: replicante:latest
hostname: agent-{uuid}
user: replicante:replicante
working_dir: /home/replicante
restart_policy: unless-stopped
```

### Volume Mounts
```yaml
volumes:
  - ./config/{agent_id}.toml:/config/agent.toml:ro
  - agent-data-{id}:/data
  - agent-workspace-{id}:/workspace
  - /tmp
```

### Security Profiles

#### Strict Profile
```yaml
security:
  privileged: false
  read_only_rootfs: true
  no_new_privileges: true
  cap_drop: [ALL]
  cap_add: []
  network_mode: none
  ipc_mode: private
  pid_mode: private
resources:
  memory: 256M
  cpu: 0.5
  pids_limit: 100
```

#### Moderate Profile
```yaml
security:
  privileged: false
  read_only_rootfs: false
  no_new_privileges: true
  cap_drop: [ALL]
  cap_add: [NET_BIND_SERVICE]
  network_mode: bridge
  ipc_mode: private
  pid_mode: private
resources:
  memory: 512M
  cpu: 1.0
  pids_limit: 200
```

#### Permissive Profile
```yaml
security:
  privileged: false
  read_only_rootfs: false
  no_new_privileges: false
  cap_drop: [SYS_ADMIN, SYS_MODULE]
  network_mode: bridge
  ipc_mode: shareable
  pid_mode: private
resources:
  memory: 1024M
  cpu: 2.0
  pids_limit: 500
```

## API Reference

### Container Management Endpoints

#### Create Agent Container
```http
POST /api/agents
Content-Type: application/json

{
  "config_path": "/config/agent.toml",
  "security_profile": "moderate",
  "resources": {
    "cpu": 1.0,
    "memory": "512M"
  }
}
```

#### Stop Agent Container
```http
POST /api/agents/{agent_id}/stop
```

#### Remove Agent Container
```http
DELETE /api/agents/{agent_id}
```

#### Get Container Stats
```http
GET /api/agents/{agent_id}/stats
```

#### Stream Container Logs
```http
GET /api/agents/{agent_id}/logs?follow=true&tail=100
```

## Testing Strategy

### Unit Tests
- Container configuration building
- Docker API error handling
- Resource limit validation
- Security profile application

### Integration Tests
```rust
#[tokio::test]
async fn test_container_lifecycle() {
    // Test container creation, start, stop, remove
}

#[tokio::test]
async fn test_resource_limits() {
    // Verify CPU and memory limits are enforced
}

#[tokio::test]
async fn test_network_isolation() {
    // Verify network policies are applied
}
```

### Security Tests
- Privilege escalation attempts
- Container escape attempts
- Resource exhaustion attacks
- Network breakout tests

## Deployment

### Prerequisites
- Docker Engine 20.10+
- Docker Compose 2.0+ (optional)
- Linux kernel 5.10+ (for advanced security features)

### Quick Start
```bash
# Build supervisor image
docker build -f Dockerfile.supervisor -t replicante-supervisor:latest .

# Run supervisor
docker run -d \
  --name replicante-supervisor \
  -p 8080:8080 \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v ./config:/config \
  replicante-supervisor:latest
```

### Production Deployment
```bash
# Use docker-compose for production
docker-compose -f docker-compose.yml up -d

# Scale agents
docker-compose -f docker-compose.yml scale agent=5
```

## Current Status

**Last Updated**: 2025-08-17

### Completed
- ✅ Initial supervisor module structure
- ✅ Basic monitoring system
- ✅ Web dashboard skeleton
- ✅ Security scanner for containers
- ✅ Docker integration with bollard crate
- ✅ Container manager module with full Docker API integration
- ✅ Container-based agent spawning (replaced process spawning)
- ✅ Container lifecycle management (start/stop/kill/pause/restart/remove)
- ✅ Docker stats integration for real-time resource monitoring
- ✅ Security profiles implementation (Strict/Moderate/Permissive)
- ✅ Container configuration with volumes, networks, and resource limits
- ✅ Integration tests for supervisor operations
- ✅ REST API endpoints for agent management (spawn, list, stop, kill, remove)
- ✅ HTTP client module for supervisor communication
- ✅ CLI commands integrated with supervisor client

### In Progress
- 🔄 Real-world testing with Docker environment
- 🔄 Container log streaming implementation

### Blocked
- ❌ None currently

### Next Steps
1. Test supervisor CLI commands with actual Docker daemon
2. Build and push agent Docker images
3. Implement real container log streaming (currently placeholder)
4. Add container event monitoring for better status tracking
5. Implement Prometheus metrics export
6. Add container exec functionality
7. Test security isolation in practice

## Notes and Decisions

### Why Bollard?
- Most mature async Docker client for Rust
- Good documentation and examples
- Active maintenance
- Supports all Docker API features we need

### Why Container-Only?
- Complete isolation between agents
- Consistent environment regardless of host
- Easy resource management
- Better security boundaries
- Simplified deployment

### Future Enhancements
- Kubernetes support for multi-host deployment
- Container image registry integration
- Auto-scaling based on load
- Distributed supervisor with consensus
- GPU support for ML workloads

## References

- [Bollard Documentation](https://docs.rs/bollard)
- [Docker Engine API](https://docs.docker.com/engine/api/)
- [Container Security Best Practices](https://docs.docker.com/develop/security-best-practices/)
- [Resource Management in Docker](https://docs.docker.com/config/containers/resource_constraints/)