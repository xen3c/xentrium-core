# Log Analysis Guide — Xentrium Testnet

How to read `xentrium.log` (or stdout). Log format:
`<RFC3339-UTC> <LEVEL> <target>: <message>`
Levels: TRACE (rustls internals — noisy, ignore unless debugging TLS),
DEBUG, INFO (operational), WARN, ERROR.

## Expected Messages (normal operation)

### Startup sequence
```
INFO  Starting Xentrium node: <node_id>
INFO  Database schema initialized                          (if storage enabled)
INFO  TCP transport started on 0.0.0.0:<port>
INFO  TLS transport enabled (cert CN = xnt-<id>)
INFO  Node started successfully: <node_id>
INFO  Starting background networking tasks
INFO  Starting heartbeat loop (30s interval)
```

### Connection formation
```
INFO  Connecting to bootstrap peers: ["<ip>:<port>"]        (outbound side)
INFO  Connected to bootstrap peer: <ip>:<port>
INFO  Accepted connection from: <ip>:<port>                 (inbound side)
INFO  Verified handshake from <ip>:<port>: node_id=xnt-... (TLS-bound)
```

### Steady state (every ~30 s per peer)
```
INFO  Heartbeat: sent Ping to <ip>:<port> (id=N)
INFO  Heartbeat: <N> active connections
INFO  Received message from <ip>:<port>: Pong
INFO  Heartbeat: Pong from <ip>:<port>, latency <dur>
```

### Protocol traffic (periodic)
```
INFO  Processing PeerList from <ip>:<port>
INFO  PeerList: <N> valid announcements, added <M> peers
INFO  Processing ValidatorRegistration / CommitteeSelection / SnapshotOffer
INFO  Registered validator: <address> (signed by xnt-...)
INFO  Updated committee with <N> members for state <S> (signed by xnt-...)
INFO  Received newer snapshot offer: state <N>
INFO  Received and stored snapshot
```

### Shutdown
```
INFO  Stopping node: <node_id>
INFO  Node stopped successfully: <node_id>
```

## Warning Messages (investigate, not necessarily fatal)

| Pattern | Meaning | Action |
|---------|---------|--------|
| `Accept error:` | transient accept failure; loop retries | single occurrence OK; repeating → check firewall/FD limits |
| `Receive error from <addr>` | connection dropped mid-read | occasional OK (peer restart); frequent → see Incident Guide |
| `Removing failed connection <addr>` | peer dropped (heartbeat miss or error) | occasional OK; loops → peer/flapping issue |
| `Heartbeat: <peer> ... Suspect/Unhealthy` | missed pongs | 1–2 misses transient; sustained → connectivity issue |
| `Failed to load config ... using defaults` | TOML parse issue at startup | verify config file; node runs on defaults (wrong ports/peers likely) |
| ` gossip`/`discovery` add failures | duplicate/invalid peer entries | benign if count is small |

## Critical Messages (act immediately)

| Pattern | Severity | Meaning |
|---------|----------|---------|
| `Rejecting peer ... invalid handshake, cert/identity mismatch, not allowlisted, or replay` | HIGH | handshake failed verification — investigate source IP; if it's a known node, check clock skew (>5 min breaks freshness), identity file corruption, or allowlist |
| `Rejecting peer ... malformed handshake` | HIGH | non-Xentrium or corrupted client on the port — possible scan/probe |
| `Rejected ValidatorRegistration/CommitteeSelection ... malformed envelope` | MEDIUM | unsigned/forged consensus message — expected rejection working; note source |
| `TLS accept failed` / `TLS handshake failed` | HIGH | TLS-layer failure (bad cert, non-TLS client, version mismatch) |
| `error` level lines / panics | CRITICAL | node-level failure — capture full log, check Incident Guide |
| Repeated `Node started successfully` with no peers | MEDIUM | bootstrap unreachable — check node1 firewall/NAT |

## Useful one-liners (Linux)

```bash
# Handshake successes
grep "Verified handshake" xentrium.log | tail -20

# Any rejections
grep -i "reject" xentrium.log | tail -20

# Heartbeat health summary
grep "Pong from" xentrium.log | tail -10

# Errors only
grep -E "ERROR|panic" xentrium.log | tail -20

# Peer connection churn
grep -c "Accepted connection" xentrium.log
```

## Noise to Ignore

- `TRACE rustls::...` lines — TLS internals, only relevant for crypto debugging
- Repeated `Heartbeat: N active connections` — informational count
- `Received message from ...: Ping/Pong` at TRACE/DEBUG — covered by heartbeat lines at INFO
