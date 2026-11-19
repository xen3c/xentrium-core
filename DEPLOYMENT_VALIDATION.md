# Deployment Validation — First Xentrium Testnet

Validation matrix for the 3-node deployment. Each requirement maps to a
verification command and the expected result. "Local proxy" rows were verified
on a single host running the real binary against the shipped configs;
physical deployment rows are executed by the operator per the runbook.

## Validation Matrix

| # | Requirement | How to verify | Expected |
|---|-------------|---------------|----------|
| 1 | Node 1 startup (node1) | `xentrium --config node1.toml --doctor` then start | `DOCTOR: ALL CHECKS PASSED`; `Node started successfully` |
| 2 | Node 2 startup (node2) | doctor + start | same |
| 3 | Node 3 startup (node3) | doctor + start | same |
| 4 | Discovery | node2/node3 logs: `Connected to bootstrap peer` | outbound conn to node1 established |
| 5 | TLS handshake | logs: `TLS transport enabled (cert CN = xnt-...)` + `Verified handshake ... (TLS-bound)` | mutual TLS on every conn; no plaintext fallback |
| 6 | Heartbeat | logs every 30 s: `Heartbeat: sent Ping` / `Pong ... latency` | peers stay Healthy; no mass removals |
| 7 | Peer synchronization | gossip convergence: node1 learns node2+node3 endpoints via signed PeerList announcements | each node ≥2 peers |
| 8 | Validator propagation | `broadcast_validator_registration` path exercised by maintenance loop; signed envelope verified | `Registered validator:` log lines, no `Rejected ... envelope` |
| 9 | Committee synchronization | `CommitteeSelection` signed envelope → `Updated committee with N members` | converged committee state on all nodes |
| 10 | Snapshot synchronization | `SnapshotOffer`/`SnapshotData` messages; `sync.get_current_state_number()` converges | snapshot metadata propagated; state numbers equal |
| 11 | Persistence | CTRL+C → restart → logs show restored validators/committee/peers/snapshots | state survives restart (verified by `persistence_audit` + `durability` tests) |
| 12 | Graceful shutdown | CTRL+C → `Node shutdown complete` | clean exit, DB released |

## Local Proxy Evidence (this host)

The following were executed against the real implementation before packaging:

- `cargo test` — **200/200 tests pass**, including:
  - `testnet_deployment.rs::test_three_node_testnet_deployment` — 3 independent
    `Node` instances start, discover via bootstrap, heartbeat, propagate
    validator registration and committee selection, init snapshot sync, shut down.
  - `security_remediation.rs` — 11 tests covering identity binding, allowlist,
    signed envelopes, gossip announcements, replay/freshness.
  - `transport_security.rs` — 6 tests: mutual TLS handshake, cert validation,
    invalid-cert rejection, encrypted Ping/Pong.
  - `heartbeat.rs`, `durability.rs`, `persistence_audit.rs` — liveness,
    crash/restart recovery, disk durability.
- `cargo build --release -p xentrium-node` — release binary produced.
- `xentrium --doctor` — pre-flight checks implemented and exercised.

## Physical Deployment Checklist (operator)

- [ ] All three nodes pass `--doctor`
- [ ] `<NODE1_PUBLIC_IP>` replaced in node2/node3 configs
- [ ] node1 firewall/security group allows inbound TCP 30303
- [ ] Node1 started first; node2/node3 within ~60 s
- [ ] Each log shows `Verified handshake ... (TLS-bound)` for every peer
- [ ] Heartbeat lines repeat every 30 s on all nodes
- [ ] No `Rejecting peer`, `TLS accept failed`, or `Accept error` after startup
- [ ] (Recommended) `allowed_node_ids` populated and nodes restarted
- [ ] CTRL+C shutdown verified on at least one node; restart resumes state

## Known Limitations Carried Into Deployment

- TCP transport (not QUIC) — TAD QUIC migration remains future work.
- Custom bincode message serialization (not Protocol Buffers).
- Bootstrap consensus phase applies (<21 active validators) — expected.
- Rate limiting absent (M1) — acceptable for 3-node invite-only testnet.
- `machine_id` field self-declared (M5) — not used for auth decisions.
