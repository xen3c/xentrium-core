# Incident Response Guide — First Xentrium Testnet

Playbooks for the incidents most likely in a 3-node testnet. For each: detect →
diagnose → mitigate → verify. Always capture logs BEFORE restarting nodes.

General rules:
- Never delete `data/<node>/` — move it aside if state is suspect.
- Restart order for recovery: node1 → node2 → node3.
- CTRL+C is the only clean stop; `Stop-Process`/kill loses in-flight state writes
  (SQLite WAL makes this survivable, but prefer graceful).

---

## 1. Node Offline (one node stops responding)

**Detect:** heartbeat Pong lines stop for that peer; other nodes log
`Removing failed connection <addr>` after ~2 missed cycles; or node process gone.

**Diagnose:**
```bash
# On the affected node
ps aux | grep xentrium            # Linux
tasklist | findstr xentrium       # Windows
tail -50 data/<node>/xentrium.log # last log lines before death
```

**Common causes:** process crash (check for `panic`/`ERROR` in log), OS
sleep/hibernate (node3), OOM, disk full, accidental CTRL+C.

**Mitigate:**
- node3: disable sleep on AC (`powercfg` / OS settings) — sleep kills heartbeat.
- Disk full: `df -h`; archive old logs, vacuum is NOT needed (WAL auto-checkpoints).
- Crash: capture log, restart node; it resumes from `xentrium.db`.

**Verify:** `Connected to bootstrap peer` + `Verified handshake` reappear;
heartbeat Pong resumes within ~1 min.

---

## 2. Handshake Failure (`Rejecting peer ...`)

**Detect:** repeated `Rejecting peer <addr>: invalid handshake, cert/identity
mismatch, not allowlisted, or replay` or `malformed handshake`.

**Diagnose — identify which sub-check failed:**
| Log says | Cause | Fix |
|----------|-------|-----|
| `cert/identity mismatch` | TLS cert CN ≠ handshake node_id — happens if `node.identity` and `tls/` came from different nodes (copied data dir!) | Ensure each node's `data_dir` is unique and was generated locally |
| `not allowlisted` | `allowed_node_ids` doesn't contain that node_id | Add the node's `xnt-...` id (from its doctor output) to all configs, restart |
| `invalid handshake` (generic) | bad signature — corrupted message or wrong key | Rare; capture packet/log context |
| `replay` | duplicate (node_id, nonce) — stale/replayed handshake | Should self-resolve on reconnect (new nonce); persistent → MITM/probe, block source IP |
| timestamp-related | **clock skew > 5 min** — handshake freshness window is ±300 s | Sync clocks: `w32tm /resync` (Win), `timedatectl set-ntp true` (Linux), `sntp -sS time.google.com` (macOS) |

**Verify:** after fix, `Verified handshake ... (TLS-bound)` appears and stays.

---

## 3. Peer Removal / Flapping

**Detect:** `Removing failed connection <addr>` repeatedly for the same peer;
`Heartbeat: <peer> Unhealthy`; peer count oscillating.

**Diagnose:**
- Latency climbing before removal? (`Pong ... latency` rising → congestion/loss)
- One-directional? Check if the *remote* node also drops *you* — asymmetric
  failure suggests NAT/firewall killing idle or long-lived conns.
- node1 under load? `htop`/`top`, `dmesg | tail` for OOM.

**Mitigate:**
- NAT timeouts: some routers kill TCP sessions; heartbeat every 30 s should
  keep them alive — if interval was raised, lower it.
- Increase `peer_timeout_secs` in `[network]` if the link is genuinely slow
  (e.g., high-latency international path) — trade-off: slower dead-peer detection.
- If a peer is permanently gone, remove it from `bootstrap_peers` on dependents.

**Verify:** stable `Heartbeat: N active connections` for 10+ min.

---

## 4. Snapshot Sync Failure

**Detect:** `SnapshotData` deserialization errors; `receive_snapshot` failures;
state numbers diverging between nodes; `Received newer snapshot offer` repeating
without progress.

**Diagnose:**
```bash
grep -E "Snapshot|snapshot" data/<node>/xentrium.log | tail -30
```
- `Snapshot deserialization failed` → corrupted/truncated payload (network or bug).
- Offer newer but no Data → peer may not have the snapshot; check sender logs.
- State numbers stuck different → late-joiner never synced; check its
  `snapshot_sync` state via logs.

**Mitigate:**
- Restart the lagging node — on startup it re-requests/adopts latest snapshot.
- If a node is >1 snapshot behind and offers keep failing, restart the *serving*
  node too (its snapshot repo may be corrupt — check `xentrium.db` integrity:
  `sqlite3 xentrium.db "PRAGMA integrity_check;"`).
- Last resort: wipe the lagging node's `data_dir` (new identity! update allowlist).

**Verify:** all nodes log the same latest `state <N>` in snapshot lines.

---

## 5. Validator / Committee Divergence

**Detect:** `Registered validator` / `Updated committee` lines show different
member sets or state numbers across nodes; `Rejected ... envelope` spikes.

**Diagnose:**
- Divergence right after a node rejoin → it missed gossip while offline;
  PeerList + snapshot sync should heal it — wait one gossip interval.
- Persistent divergence → check each node's `validator_manager` state via logs;
  compare `state_number` values.
- `Rejected CommitteeSelection` on one node only → that node's connection
  identity didn't match the signer (possible stale `node.identity` after data-dir
  copy) — see handshake failure §2.

**Mitigate:**
- Restart the diverged node; on reconnect it receives current committee via
  gossip/snapshot.
- If divergence persists >1 gossip cycle, escalate: capture all three logs,
  diff validator/committee lines, check for split-brain (two meshes — see below).

**Verify:** identical `Updated committee with N members for state S` on all nodes.

---

## 6. Split-Brain (two separate meshes)

**Detect:** nodes form two clusters — e.g., node2+node3 mesh and node1 isolated,
or duplicate `node_id`s after a data-dir copy.

**Diagnose:** each node's peer count < expected; `Connected to bootstrap peer`
missing on the isolated side; check for identical `node_id` values across
nodes (copied `data_dir` accident — each node MUST have unique identity).

**Mitigate:**
- Ensure each node generated its own `node.identity` (delete copied
  `data/<node>/node.identity` + `tls/` on the duplicate and restart — it will
  regenerate; update `allowed_node_ids` accordingly).
- Restart the isolated node after connectivity restored; bootstrap reconnects it.

**Verify:** single mesh — every node reaches every other node (directly or via gossip).

---

## Evidence Collection (before any restart)

```bash
# Per node
tail -500 data/<node>/xentrium.log > incident-<node>-$(date +%s).log
ls -la data/<node>/            # identity, tls, db files present?
sqlite3 data/<node>/xentrium.db "PRAGMA integrity_check;"
```

Attach all three logs + config.toml files to the incident report.
