# Testnet Verification Checklist

Use this checklist to verify a running 3-node Xentrium testnet.
Each item maps to observable log output or test evidence.

---

## 1. Startup Verification

- [ ] `xentrium` binary builds: `cargo build --release -p xentrum-node` (note: `xentrium-node`)
- [ ] Node 1 starts with `config/testnet-node1.toml`
- [ ] Log shows `Node started successfully: testnet-node-1`
- [ ] Node 2 starts and logs `Connecting to bootstrap peer ...`
- [ ] Node 3 starts and connects to both bootstrap peers
- [ ] Each node creates `./data/testnet-nodeN/xentrium.db`

**Evidence:** startup log lines; `xentrium.db` files exist.

---

## 2. Peer Verification

- [ ] Node logs show `Accepted connection from: 127.0.0.1:PORT` (or public IP)
- [ ] `Heartbeat: N active connections` reports N ≥ 1 on each node
- [ ] `Peer discovery: N known peers` grows as gossip propagates PeerLists
- [ ] No repeated `Accept error` or `Receive error` spam in logs

**Evidence:** connection-accepted log lines; heartbeat count lines.

---

## 3. Heartbeat / Liveness Verification

- [ ] Heartbeat loop logs every 30s (or configured `heartbeat_interval_secs`)
- [ ] No `Heartbeat: removing unhealthy peer` for healthy nodes
- [ ] If a node is stopped, peers log its removal within `peer_timeout_secs` (120s default)
- [ ] Restarted node rejoins and is not flagged unhealthy

**Evidence:** regular heartbeat log lines; stale-peer removal only on real outages.

---

## 4. Committee Verification

- [ ] Validator registrations propagate: `Processing ValidatorRegistration`
- [ ] `Registered validator: <address>` appears on receiving nodes
- [ ] `Processing CommitteeSelection` / `Updated committee with N members` appear after committee broadcast
- [ ] All nodes report identical committee membership for the same `state_number`

**Evidence:** validator/committee log lines; `get_committee_members()` parity across nodes.

---

## 5. Snapshot Verification

- [ ] `Snapshot advertisement: N available snapshots` logs on interval
- [ ] `Processing SnapshotOffer` / `Processing SnapshotData` appear when snapshots exist
- [ ] Received snapshots verified by hash before storage
- [ ] `SnapshotRepository` persists snapshot metadata to SQLite

**Evidence:** snapshot-related log lines; `snapshots` table rows.

---

## 6. Recovery Verification

- [ ] Stop a node (CTRL+C), confirm `Node stopped successfully`
- [ ] Restart same node with same config — log shows state restoration
- [ ] Validators/committee/peers/snapshots restored (no rebuild from scratch)
- [ ] Node rejoins network and heartbeats resume

**Evidence:** restart logs; `validators`, `committee_members`, `peers`, `snapshots` tables retain rows.

---

## 7. Automated Verification

The integration test `test_three_node_testnet_deployment` in
`xentrium-node/tests/testnet_deployment.rs` exercises items 1–6 in-process:

```bash
cargo test -p xentrium-node --test testnet_deployment
```

It starts 3 independent `Node` instances on distinct ports, verifies startup,
peer connections, heartbeat trackers, validator-state convergence, committee
convergence, snapshot-service initialization, and clean shutdown.

---

## Known Limitations (Testnet Scope)

- Transport is TCP, not QUIC (TAD deviation — documented, deferred)
- No TLS/encryption on P2P links
- Peer identity is endpoint-derived, not cryptographically authenticated
- Snapshot sync transfers metadata + payload but transaction replay after
  snapshot install is not yet implemented
- Scheduler intervals for discovery/gossip/snapshot advertisement are
  hardcoded; only heartbeat interval is config-driven
