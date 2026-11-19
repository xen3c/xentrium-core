# First Xentrium Testnet — Runbook

Three nodes: **node1** (seed), **node2**, and **node3**. All commands
assume the repo is built (`cargo build --release -p xentrium-node`).

Node labels used below:

- `NODE1` — node1 shell
- `NODE2` — node2 shell
- `NODE3` — node3 shell

---

## 0. Pre-flight (all nodes)

```bash
# NODE1
./target/release/xentrium --config config/deploy/node1.toml --doctor
# NODE2
.\target\release\xentrium.exe --config config\deploy\node2.toml --doctor
# NODE3
./target/release/xentrium --config config/deploy/node3.toml --doctor
```

All three must print `DOCTOR: ALL CHECKS PASSED`.
Doctor also **provisions** `node.identity` and TLS credentials and prints the
node's `node_id` (`xnt-...`) — record these; they are needed for the invite-only
allowlist in step 5.

## 1. Fill in node1 public IP

On `NODE2` and `NODE3`, edit `config/deploy/node2.toml` and
`node3.toml`: replace `<NODE1_PUBLIC_IP>` in `bootstrap_peers` with
node1's public IPv4 (find it: `curl ifconfig.me` on node1).

Re-run doctor on both to confirm `bootstrap peers` shows a valid address.

## 2. Launch order

Start **node1 first**, wait ~10 seconds, then node2, then node3.

```bash
# NODE1 — leave running (use tmux/screen/nohup)
./target/release/xentrium --config config/deploy/node1.toml

# NODE2
.\target\release\xentrium.exe --config config\deploy\node2.toml

# NODE3
./target/release\xentrium --config config/deploy/node3.toml
```

## 3. Verify the network formed

On each node's log you should see, within ~30–60 s:

```
INFO  TCP transport started on 0.0.0.0:3030X
INFO  TLS transport enabled (cert CN = xnt-...)
INFO  Node started successfully: testnet-...-N
INFO  Connected to bootstrap peer: <NODE1_IP>:30303        (node2/node3)
INFO  Accepted connection from: ...                     (node1)
INFO  Verified handshake from ...: node_id=xnt-... (TLS-bound)
INFO  Heartbeat: sent Ping to ...
INFO  Heartbeat: Pong from ..., latency ...
```

Positive checks:

- `connections()` equivalent — every node should report ≥2 peers once gossip
  converges (node1 sees node2+node3; node2/node3 see each other via
  PeerList announcements).
- No `Rejecting peer` / `TLS accept failed` / `Accept error` lines after startup.
- `Heartbeat` lines repeat every 30 s without stale-peer removals.

Validator/committee/snapshot propagation is exercised automatically by the
maintenance loops (validator registration broadcast, committee selection,
snapshot offers every interval). With <21 active validators the network stays
in **Bootstrap consensus phase** per TAD v2.2.7 — this is expected.

## 4. Invite-only lockdown (optional but recommended)

Collect the three `node_id`s (`xnt-...`) printed by doctor/startup logs, then on
**every** node set:

```toml
[network]
allowed_node_ids = ["xnt-<id1>", "xnt-<id2>", "xnt-<id3>"]
```

Restart all nodes (CTRL+C, wait for `Node shutdown complete`, start again).
After restart, only listed identities can complete handshakes — all others are
disconnected at the handshake stage.

## 5. Shutdown

`CTRL+C` on each node. Confirm `Node shutdown complete` in logs.
State persists in each `data_dir`; restart resumes from disk.

## 6. Troubleshooting quick map

| Symptom | See |
|---------|-----|
| `port availability` FAIL in doctor | FIRST_TESTNET_GO_LIVE.md §Firewall |
| `Connected to bootstrap peer` never appears | FIRST_TESTNET_GO_LIVE.md §NAT/Bootstrap |
| `TLS accept failed` / handshake loops | FIRST_TESTNET_GO_LIVE.md §TLS identity |
| Peers connect then drop | FIRST_TESTNET_GO_LIVE.md §Replay/allowlist |
