# Real Deployment Checklist — First Xentrium Testnet

Operator checklist for launching the 3-node testnet (node1 + node2 + node3).
Work top to bottom; do not skip gates.

## A. Build & Package (per node)

- [ ] Rust stable toolchain installed (`rustc --version` ≥ 1.7x)
- [ ] Repo/workspace present on each node
- [ ] `cargo build --release -p xentrium-node` completed without errors
- [ ] Binary exists: `target/release/xentrium` (Linux) / `xentrium.exe` (Windows)

## B. Configuration (per node)

- [ ] Correct config copied: `node1.toml` / `node2.toml` / `node3.toml`
- [ ] `<NODE1_PUBLIC_IP>` replaced with real public IPv4 on node2 + node3
- [ ] `data_dir` paths are distinct per node
- [ ] `allowed_node_ids` left empty for first boot (lockdown is step H)
- [ ] `logging.file` path is writable

## C. Pre-flight (per node — GATE)

- [ ] `xentrium --config <cfg> --doctor` → `DOCTOR: ALL CHECKS PASSED`
- [ ] Node `node_id` (`xnt-...`) recorded for each node — needed for lockdown
- [ ] node1: inbound TCP **30303** open in OS firewall AND external security group
- [ ] node2/node3: outbound TCP unrestricted (default)

## D. Launch (ordered — GATE)

- [ ] **Node 1 (node1) started first**, `Node started successfully` in log
- [ ] Wait ≥10 s
- [ ] **Node 2 (node2) started**, `Connected to bootstrap peer` appears
- [ ] **Node 3 (node3) started**, `Connected to bootstrap peer` appears
- [ ] Wait ~60 s for gossip convergence

## E. Formation Verification (GATE — all must pass)

- [ ] Every node log shows `Verified handshake from ... (TLS-bound)` per peer
- [ ] Every node shows repeating `Heartbeat: sent Ping` / `Pong ... latency` (~30 s)
- [ ] No `Rejecting peer`, `TLS accept failed`, or repeating `Accept error`
- [ ] PeerList gossip converged: each node knows the other two
- [ ] Validator/committee propagation lines present, zero `Rejected` envelope logs
- [ ] Snapshot offers flowing (`SnapshotOffer`/`SnapshotData` messages in logs)

## F. Persistence Spot-Check

- [ ] `data/<node>/node.identity`, `tls/tls_credentials.bin`, `xentrium.db` exist on each node
- [ ] (Optional) CTRL+C one node → restart → state restored, peers reconnect

## G. Invite-Only Lockdown (recommended before public announcement)

- [ ] `allowed_node_ids = ["xnt-A", "xnt-B", "xnt-C"]` set on ALL nodes
- [ ] All nodes restarted (CTRL+C → `Node shutdown complete` → start)
- [ ] Post-restart: three `Verified handshake` lines reappear; foreign connectors rejected

## H. Handoff to Observation Period

- [ ] All nodes running under a supervisor (tmux/screen/Task Scheduler/NSSM)
- [ ] Logs rotating or monitored (see LOG_ANALYSIS_GUIDE.md)
- [ ] FIRST_24H_OBSERVATION_CHECKLIST.md schedule posted to operators
- [ ] Incident contacts + INCIDENT_RESPONSE_GUIDE.md distributed

## Rollback Trigger

Any of: repeated `Accept error` loop, all peers flapping, state divergence,
TLS failures after restart → execute rollback in FIRST_TESTNET_GO_LIVE.md §Rollback.
