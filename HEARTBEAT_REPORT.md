# Heartbeat System Report

**Date:** 2026-11-16
**Phase:** PHASE-3 STEP-3 - Real Heartbeat System
**Protocol Version:** TAD v2.2.7 (Frozen)

---

## Summary

The heartbeat framework has been converted into a fully operational liveness system. Nodes now actively send Ping messages on a configurable interval, track round-trip latency per peer, classify peers as Healthy/Suspect/Unhealthy, automatically remove unhealthy peers, and recover peers automatically after reconnect.

---

## Files Created

| File | Purpose |
|------|---------|
| `xentrium-network/src/heartbeat.rs` | `HeartbeatTracker`, `PeerLiveness`, `PeerHealth` enum — full liveness tracking engine |
| `xentrium-node/tests/heartbeat.rs` | 7 integration tests covering ping, pong, latency, health transitions, removal, recovery |
| `HEARTBEAT_REPORT.md` | This report |

## Files Modified

| File | Changes |
|------|---------|
| `xentrium-network/src/lib.rs` | Exported `heartbeat` module and its types |
| `xentrium-node/src/lib.rs` | Added `heartbeat_tracker: Arc<RwLock<HeartbeatTracker>>` field to `Node`; added `heartbeat_interval_secs`/`peer_timeout_secs` to `NodeConfig`; initialized tracker in both constructors; rewrote heartbeat loop to actively send Ping, check misses, remove unhealthy; Pong handler now records latency via tracker; added `heartbeat_tracker()` getter |
| `xentrium-node/src/main.rs` | Wired `heartbeat_interval_secs` and `peer_timeout_secs` from TOML config into `NodeConfig` |
| `xentrium-node/tests/multi_node.rs` | Added new NodeConfig fields to helper |
| `xentrium-node/tests/durability.rs` | Added new NodeConfig fields to helper |
| `xentrium-node/tests/persistence_audit.rs` | Added new NodeConfig fields to helper |
| `xentrium-node/tests/node_operation.rs` | Added new NodeConfig fields to helper |

---

## Features Implemented

### 1. Active Ping Transmission
- Heartbeat loop (spawned in `start_networking_tasks`) now sends `MessageType::Ping` to every connected peer on each interval
- Ping payload carries a monotonically increasing ping ID (`u64`) so pongs can be correlated
- Ping send failures are logged; peer remains tracked

### 2. Pong Response (pre-existing, now leveraged)
- Receiving node already responds to `Ping` with `Pong` in the message loop
- Pong handler now additionally records the pong in `HeartbeatTracker` and logs measured latency

### 3. Liveness Tracking (`PeerLiveness`)
| Field | Description |
|-------|-------------|
| `last_ping_sent` | Timestamp of most recent Ping |
| `last_pong_received` | Timestamp of most recent Pong |
| `last_ping_id` | Correlation ID of outstanding ping |
| `consecutive_misses` | Count of heartbeat cycles with no pong |
| `consecutive_successes` | Count of successful ping/pong cycles |
| `last_latency` | Most recent round-trip latency |
| `avg_latency` | Exponential moving average (α=0.3) of latency |
| `health` | `Healthy` / `Suspect` / `Unhealthy` |

### 4. Health Classification
- **Healthy**: 0 consecutive misses, pong within timeout
- **Suspect**: exactly 1 consecutive miss, OR elevated avg latency (> heartbeat interval) with recent misses
- **Unhealthy**: ≥2 consecutive misses, OR no pong received within `peer_timeout_secs`

### 5. Automatic Removal
- `HeartbeatTracker::check_missed()` runs each heartbeat cycle
- Newly-unhealthy peers are removed from `Discovery` and their TCP connections dropped
- Removal is logged: `Heartbeat: removing unhealthy peer ...`

### 6. Automatic Recovery
- When a pong arrives from a previously-unhealthy peer, `record_pong()` resets `consecutive_misses` and `update_health()` restores `Healthy`
- Verified by `test_peer_recovery_after_reconnect`

### 7. Configuration Wiring
- `NodeConfig` gained `heartbeat_interval_secs` (default 30) and `peer_timeout_secs` (default 120)
- `main.rs` maps `network.heartbeat_interval_secs` and `network.connection_timeout_secs` from `config.toml`
- `HeartbeatTracker::new(interval, timeout)` and `update_config()` support runtime reconfiguration
- Heartbeat loop sleep interval now uses the configured value instead of hardcoded 30s

---

## Design Detail

```
HeartbeatTracker
├── peers: HashMap<PeerID, PeerLiveness>
├── next_ping_id: u64
├── heartbeat_interval: Duration
└── peer_timeout: Duration

Loop (every heartbeat_interval_secs):
  1. Register all connected peers (get_or_create)
  2. peers_due_for_ping() -> Vec<(PeerID, ping_id)>
  3. Send Ping(ping_id) to each due peer
  4. Log connection count
  5. check_missed() -> Vec<PeerID> newly unhealthy
  6. Remove unhealthy from discovery + connections

On Pong received:
  record_pong(peer_id) -> Option<(PeerID, latency)>
  → resets misses, updates EMA latency, recomputes health
```

**Miss detection:** a peer is marked missed when a ping was sent and `heartbeat_interval` elapsed without a pong. Two misses → Unhealthy. `peer_timeout_secs` provides a hard ceiling: if `last_pong_received` is older than timeout, peer is Unhealthy regardless of miss count.

---

## Integration Tests Added (7/7 passing)

| Test | Verifies |
|------|----------|
| `test_heartbeat_ping` | Peer registered → due for ping |
| `test_heartbeat_pong_latency` | Pong records latency, health=Healthy |
| `test_latency_tracking` | EMA avg latency over 3 cycles |
| `test_unhealthy_peer_detection` | 1 miss → Suspect, 2 misses → Unhealthy |
| `test_stale_peer_removal` | Unhealthy peer removable from tracker |
| `test_peer_recovery_after_reconnect` | Pong after Unhealthy → Healthy |
| `test_heartbeat_with_two_nodes` | Two live nodes, tracker initialized, clean shutdown |

Plus 4 unit tests inside `heartbeat.rs` (ping/pong latency, unhealthy detection, health transitions, peer recovery).

---

## Cargo Output / Test Results

```
xentrium-consensus: 24/24
xentrium-crypto: 8/8
xentrium-network: 43 unit + 20 multi-node + 5 discovery + 6 gossip + 6 sync + 3 tcp = 83
xentrium-node: 6 unit + 25 multi_node + 6 durability + 6 node_operation + 7 heartbeat + 1 persistence_audit + 1 transport = 52
xentrium-shared: 0
xentrium-storage: 0

Total: 167/167 tests passing
```

Heartbeat-specific: 7/7 integration + 4/4 unit tests passing.

---

## Constraints Compliance

- ✅ No protocol changes (Ping/Pong message types pre-existing)
- ✅ No consensus changes
- ✅ No QUIC work (TCP retained)

---

## Known Limitations

1. **Peer identity in tracker is endpoint-derived** — `PeerID::new(format!("peer-{}", addr), addr, addr.to_string())` synthesizes IDs from socket addresses. When real peer identity (public key / machine ID) is exchanged in handshake, tracker keys should switch to authenticated IDs.
2. **Ping ID correlation is implicit** — pongs don't echo the ping ID; matching is per-connection/peer. Adequate on a single connection per peer.
3. **Heartbeat interval config loaded but scheduler loops for discovery/gossip/snapshot remain hardcoded** — only heartbeat interval is wired.
4. **No jitter** — all peers are pinged in lockstep each cycle; production would add per-peer jitter to avoid thundering herd.

---

## Success Criteria Assessment

| Criterion | Status |
|-----------|--------|
| Nodes continuously verify peer liveness without manual intervention | ✅ Heartbeat loop autonomously pings, tracks, classifies, removes |

**PHASE-3 STEP-3: COMPLETE**
