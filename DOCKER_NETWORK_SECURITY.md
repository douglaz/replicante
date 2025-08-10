# Docker Network Security for Replicante

## Overview

This document describes the **infrastructure-level** network security implementation for Replicante. Unlike application-level sandboxing (which requires bot cooperation), this approach enforces security at the Docker/network level, making it impossible for the bot to bypass.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Host System                           │
├─────────────────────────────────────────────────────────┤
│                  Docker Network                          │
│                   172.20.0.0/16                         │
│                                                          │
│  ┌──────────┐     ┌──────────┐     ┌──────────┐       │
│  │   DNS    │────▶│  Proxy   │────▶│ Internet │       │
│  │172.20.0.3│     │172.20.0.2│     │          │       │
│  └──────────┘     └──────────┘     └──────────┘       │
│       ▲                ▲                                │
│       │                │                                │
│  ┌──────────┐     ┌──────────┐                        │
│  │  Agent   │────▶│Supervisor│                        │
│  │172.20.0.x│     │172.20.0.5│                        │
│  └──────────┘     └──────────┘                        │
│                                                          │
│  ┌──────────────────────────┐                          │
│  │    Network Monitor        │                          │
│  │  (Observes all traffic)   │                          │
│  └──────────────────────────┘                          │
└─────────────────────────────────────────────────────────┘
```

## Security Layers

### 1. DNS Filtering (dnsmasq)
- **Location**: `dns/dnsmasq.conf`
- **Function**: Only resolves whitelisted domains
- **Enforcement**: Returns NXDOMAIN for all non-whitelisted domains
- **Cannot be bypassed**: Agent must use this DNS server

### 2. HTTP/HTTPS Proxy (Squid)
- **Location**: `proxy/squid.conf`
- **Function**: Only allows HTTP/HTTPS to whitelisted domains
- **Enforcement**: Blocks all other requests at proxy level
- **Cannot be bypassed**: Docker networking forces all HTTP through proxy

### 3. Docker Network Isolation
- **Configuration**: `docker-compose.network.yml`
- **Function**: Isolated bridge network
- **Enforcement**: No direct internet access, must go through proxy/DNS

### 4. Container Security
- **Read-only root filesystem**
- **Dropped Linux capabilities**
- **No new privileges**
- **Resource limits (CPU/Memory)**

### 5. Network Monitoring
- **Rust Binary**: `src/bin/network-monitor.rs`
- **Bash Scripts**: `network-policy/*.sh`
- **Function**: Observes and reports violations
- **Does NOT enforce**: Pure monitoring/alerting

## What Gets Blocked

| Layer | Blocks | How |
|-------|--------|-----|
| DNS | Domain resolution | Returns 0.0.0.0 for blocked domains |
| Proxy | HTTP/HTTPS requests | Returns 403 Forbidden |
| Docker | Direct connections | Network isolation |
| iptables (optional) | All non-whitelisted | DROP packets |

## Allowed Connections

Only these are allowed by default:
- `api.anthropic.com` (AI provider)
- `api.openai.com` (AI provider)
- Internal Docker network (172.20.0.0/16)
- Supervisor API

## Usage

### Starting the Secure Environment

```bash
# Start all services with network security
docker-compose -f docker-compose.yml -f docker-compose.network.yml up -d

# The agent container will automatically:
# 1. Use DNS server at 172.20.0.3
# 2. Route HTTP/HTTPS through proxy at 172.20.0.2
# 3. Be monitored by network-monitor
```

### Monitoring

```bash
# View proxy logs
docker logs replicante-proxy

# View DNS logs
docker logs replicante-dns

# View network monitor logs
docker logs replicante-netmon

# Check blocked connections
docker exec replicante-proxy tail -f /var/log/squid/access.log | grep DENIED
```

### Adding Allowed Domains

1. Edit `proxy/whitelist.txt` to add domains
2. Edit `dns/dnsmasq.conf` to add DNS resolution
3. Restart services:
```bash
docker-compose -f docker-compose.network.yml restart proxy dns
```

## Security Properties

### What This Prevents

- ✅ **Data exfiltration**: Can't connect to unauthorized servers
- ✅ **Command & Control**: Can't reach malicious C&C servers
- ✅ **Lateral movement**: Isolated from other containers/host
- ✅ **DNS tunneling**: DNS queries are filtered
- ✅ **Direct IP connections**: Must use proxy

### What This Doesn't Prevent

- ❌ **Allowed domain abuse**: If api.anthropic.com is compromised
- ❌ **Resource exhaustion**: Within container limits
- ❌ **Local exploitation**: Within container

## Testing

### Test DNS Blocking
```bash
# Should fail (NXDOMAIN)
docker exec agent-secured nslookup evil.com

# Should succeed
docker exec agent-secured nslookup api.anthropic.com
```

### Test Proxy Blocking
```bash
# Should fail (403 Forbidden)
docker exec agent-secured curl https://evil.com

# Should succeed
docker exec agent-secured curl https://api.anthropic.com
```

### Test Direct Connection Blocking
```bash
# Should fail (no route to host)
docker exec agent-secured curl --noproxy '*' https://1.1.1.1
```

## Comparison with Application-Level Sandboxing

| Aspect | Application-Level | Infrastructure-Level |
|--------|------------------|---------------------|
| Enforcement | Bot must call check functions | Automatic at network level |
| Can be bypassed? | Yes (if bot doesn't call checks) | No (enforced by kernel/Docker) |
| Performance | Function call overhead | Minimal (happens anyway) |
| Complexity | Requires integration | Works transparently |
| Security | Cooperative | Mandatory |

## Advanced Configuration

### Enabling iptables (Additional Layer)

For extra security, you can add iptables rules inside containers:

```bash
# Run setup script in container
docker exec agent-secured /bin/sh /network-policy/setup-iptables.sh
```

### SSL/TLS Inspection

To inspect HTTPS traffic, configure Squid with SSL bump:
1. Generate certificates (see `proxy/squid.conf` comments)
2. Uncomment SSL bump configuration
3. Install CA certificate in agent container

### Custom Network Policies

Edit `docker-compose.network.yml` to add:
- Rate limiting
- Bandwidth limits
- Additional monitoring

## Troubleshooting

### Agent Can't Connect
1. Check DNS: `docker logs replicante-dns`
2. Check proxy: `docker logs replicante-proxy`
3. Verify domain is in whitelist

### High Memory Usage
- Adjust container limits in docker-compose.yml
- Check for memory leaks with network-monitor

### Monitoring Not Working
- Ensure network-monitor has NET_ADMIN capability
- Check `/proc` is mounted correctly

## Security Best Practices

1. **Minimal Whitelist**: Only add absolutely necessary domains
2. **Regular Updates**: Keep Docker, Squid, dnsmasq updated
3. **Log Review**: Regularly check logs for anomalies
4. **Test Restrictions**: Periodically verify blocking works
5. **Incident Response**: Have plan for security breaches

## Conclusion

This infrastructure-level approach provides robust network security without requiring any cooperation from the bot. Security is enforced at multiple layers (DNS, Proxy, Docker networking), making it extremely difficult to bypass restrictions.