# First 24-Hour Observation Checklist

Monitoring schedule for the first Xentrium testnet. Times are relative to
**T0 = all three nodes verified formed (Section E of REAL_DEPLOYMENT_CHECKLIST).**

Log sources: `data/<node>/xentrium.log` on each node (or stdout if
`logging.file` unset). Use LOG_ANALYSIS_GUIDE.md for message interpretation.

## T+0 → T+15 min — Formation Stability

- [ ] All 3 nodes: heartbeat lines every ~30 s, latency < 500 ms (node1↔node3 may be higher; flag >2 s)
- [ ] Peer count stable: each node reports its 2 peers; no `Removing failed connection` loops
- [ ] Zero `Rejecting peer` after initial handshakes settle
- [ ] Validator registration + committee selection lines appeared once, no repeats/floods

## T+15 min → T+1 h — Protocol Loops

- [ ] PeerList gossip: periodic `PeerList` processing, no announcement storms
- [ ] Snapshot advertisement loop running (interval per config; testnet default is shortened)
- [ ] No `SnapshotData` deserialization errors
- [ ] Memory stable on all nodes (no growth trend — watch for M6 gossip-table growth)

## T+1 h → T+6 h — Sustained Operation

- [ ] Heartbeat misses: zero `Unhealthy` classifications; if any peer marked Suspect→Unhealthy→removed, capture logs (incident: peer removal)
- [ ] No reconnect storms: `Connected to bootstrap peer` should appear only at startup/recovery, not repeatedly
- [ ] Disk usage stable; `xentrium.db` WAL not growing unboundedly
- [ ] Log volume reasonable (< ~50 MB/node/day at info level)

## T+6 h → T+24 h — Soak

- [ ] All three nodes still `Heartbeat: 2 active connections` (or correct peer count)
- [ ] State convergence: validator/committee views identical across nodes (compare `Registered validator` / `Updated committee` lines)
- [ ] No critical log patterns (see LOG_ANALYSIS_GUIDE.md §Critical)
- [ ] Snapshot synchronization: latest state numbers equal across nodes

## End-of-24h Sign-Off

- [ ] Fill in observation log below
- [ ] Decide: continue soak / apply lockdown / schedule maintenance window

### Observation Log

| Check | T+15m | T+1h | T+6h | T+24h | Notes |
|-------|-------|------|------|-------|-------|
| Peers connected | | | | | |
| Heartbeat healthy | | | | | |
| No rejections | | | | | |
| Validator converged | | | | | |
| Committee converged | | | | | |
| Snapshots synced | | | | | |
| Memory stable | | | | | |
| Incidents | | | | | |

## Abort Criteria (any time)

- Split-brain: nodes form two separate meshes
- Repeated handshake/TLS failures after initial formation
- Unbounded memory growth (>2× baseline and climbing)
- Any node crash-looping (`Node started` → immediate exit)
