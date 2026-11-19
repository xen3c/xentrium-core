# Xentrium Testnet Deployment Guide

**Protocol Version:** TAD v2.2.7 (Frozen)
**Transport:** TCP (QUIC deferred — see QUIC_FEASIBILITY_ASSESSMENT.md)

---

## 1. System Requirements

| Component | Minimum | Recommended |
|-----------|---------|-------------|
| OS | Windows 10+, Linux, macOS | Linux (Ubuntu 22.04+) |
| CPU | 2 cores | 4 cores |
| RAM | 4 GB | 8 GB |
| Disk | 20 GB SSD | 100 GB NVMe |
| Network | Stable public IP, open inbound port | Static IP + port forwarding |
| Rust | 1.75+ (stable) | Latest stable |

---

## 2. Ports

| Port | Purpose | Direction |
|------|---------|-----------|
| 30303 | Node 1 (bootstrap) P2P | TCP inbound+outbound |
| 30304 | Node 2 P2P | TCP inbound+outbound |
| 30305 | Node 3 P2P | TCP inbound+outbound |

Each node binds `0.0.0.0:<port>` and dials bootstrap peers outbound.

---

## 3. Firewall Rules

Allow inbound TCP on each node's P2P port:

```bash
# Linux (ufw)
sudo ufw allow 30303/tcp
sudo ufw allow 30304/tcp
sudo ufw allow 30305/tcp

# Windows (PowerShell, admin)
New-NetFirewallRule -DisplayName "Xentrium P2P 30303" -Direction Inbound -Protocol TCP -LocalPort 30303 -Action Allow
New-NetFirewallRule -DisplayName "Xentrium P2P 30304" -Direction Inbound -Protocol TCP -LocalPort 30304 -Action Allow
New-NetFirewallRule -DisplayName "Xentrium P2P 30305" -Direction Inbound -Protocol TCP -LocalPort 30305 -Action Allow
```

Outbound TCP must be allowed to reach bootstrap peers (usually open by default).

---

## 4. Bootstrap Configuration

- **Node 1** is the bootstrap/seed node: `bootstrap_peers = []`
- **Node 2** bootstraps from Node 1: `bootstrap_peers = ["<NODE1_IP>:30303"]`
- **Node 3** bootstraps from both: `bootstrap_peers = ["<NODE1_IP>:30303", "<NODE2_IP>:30304"]`

Replace `<NODE1_IP>` / `<NODE2_IP>` with the public IPs of the respective hosts in:
- `config/testnet-node2.toml`
- `config/testnet-node3.toml`

---

## 5. Node Startup Commands

Build once:

```bash
cargo build --release -p xentrium-node
```

Start each node (from repo root):

```bash
# Node 1 (bootstrap)
cargo run --release -p xentrium-node --bin xentrium -- --config config/testnet-node1.toml

# Node 2
cargo run --release -p xentrium-node --bin xentrium -- --config config/testnet-node2.toml

# Node 3
cargo run --release -p xentrium-node --bin xentrium -- --config config/testnet-node3.toml
```

Or with the compiled binary directly:

```bash
target/release/xentrium.exe --config config/testnet-node1.toml   # Windows
target/release/xentrium --config config/testnet-node1.toml       # Linux/macOS
```

### CLI Overrides

Any config value can be overridden on the command line:

```bash
xentrium --config config/testnet-node2.toml --port 30310 --log-level debug
xentrium --node-id my-node --address 0.0.0.0 --port 30303 --data-dir ./data/my-node
```

---

## 6. Database Locations

SQLite databases are created under each node's `data_dir`:

| Node | Database | WAL files |
|------|----------|-----------|
| Node 1 | `./data/testnet-node1/xentrium.db` | `xentrium.db-wal`, `xentrium.db-shm` |
| Node 2 | `./data/testnet-node2/xentrium.db` | same pattern |
| Node 3 | `./data/testnet-node3/xentrium.db` | same pattern |

Schema tables: `accounts`, `transactions`, `validators`, `snapshots`, `governance_proposals`, `committee_members`, `peers`.

---

## 7. Log Locations

Per config, logs go to `<data_dir>/xentrium.log`:

```
./data/testnet-node1/xentrium.log
./data/testnet-node2/xentrium.log
./data/testnet-node3/xentrium.log
```

Log level is set via `logging.level` in TOML (`trace`, `debug`, `info`, `warn`, `error`) or `--log-level` CLI flag.

---

## 8. Operational Commands

### Start
```bash
xentrium --config config/testnet-node1.toml &
```

### Stop (graceful)
Press `CTRL+C` in the node's terminal (SIGINT/SIGTERM on Unix, CTRL+C on Windows).
The node flushes state, clears connections, and exits cleanly.

### Restart
```bash
# stop with CTRL+C, then:
xentrium --config config/testnet-node1.toml
```
State (validators, committee, peers, snapshots) is restored from SQLite automatically.

### Inspect logs
```bash
# Live tail (Linux)
tail -f ./data/testnet-node1/xentrium.log

# Windows PowerShell
tail -f ./data/testnet-node1/xentrium.log   # or Get-Content -Wait

# Search for errors
Select-String -Path ./data/testnet-node1/xentrium.log -Pattern "ERROR|WARN"
```

### Check peer count / health
Watch the log for periodic lines:
- `Heartbeat: N active connections`
- `Peer discovery: N known peers`
- `Heartbeat: removing unhealthy peer ...`

---

## 9. Deployment Checklist (Quick)

1. Build release binary: `cargo build --release -p xentrium-node`
2. Edit `config/testnet-node{2,3}.toml` with real IPs
3. Open firewall ports 30303–30305
4. Start Node 1 first, then Nodes 2 and 3
5. Verify logs show connections, heartbeats, and peer discovery
6. See `TESTNET_CHECKLIST.md` for full verification steps
