# Xentrium Installation Guide

How to install and run a Xentrium testnet node on Windows, Linux, or macOS.

## Prerequisites

- **Rust toolchain** (stable): https://rustup.rs
  - Windows: `winget install Rustlang.Rustup` or rustup-init.exe
  - Linux/macOS: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- ~2 GB free disk (build artifacts + node data)
- Open inbound TCP port for the node's listen port (node1 only — see firewall section)

## Build from Source

```bash
git clone <repo-url> xentrium   # or copy the workspace directory
cd xentrium
cargo build --release -p xentrium-node
```

The node binary is produced at:

- Windows: `target\release\xentrium.exe`
- Linux/macOS: `target/release/xentrium`

> Build node1 directly on node1, or cross-compile for Linux with
> `cargo build --release --target x86_64-unknown-linux-gnu` from a
> host with the Linux target installed (`rustup target add x86_64-unknown-linux-gnu`).

## Configure

Copy a deployment config and edit it:

```bash
# Linux/macOS
cp config/deploy/node1.toml config.toml  # on node1
cp config/deploy/node2.toml config.toml  # on node2
cp config/deploy/node3.toml config.toml  # on node3

# Windows (PowerShell)
copy config\deploy\node1.toml config.toml
```

Required edits before launch:

1. **`<NODE1_PUBLIC_IP>`** in `bootstrap_peers` (node2/node3 configs) — replace with node1's real public IPv4.
2. **`data_dir`** — adjust if desired (default `./data/nodeN` is fine).
3. **`allowed_node_ids`** — leave empty for the first boot, then lock down (see Runbook §5).

## Pre-flight Check (doctor)

Every node should pass the built-in doctor before launch:

```bash
./target/release/xentrium --config config.toml --doctor
# Windows: .\target\release\xentrium.exe --config config.toml --doctor
```

Doctor verifies: config parses, network address valid, **port is free**,
data dir writable, node identity provisioned (prints `node_id`), TLS
certificate generated/validated, bootstrap peers parse, allowlist status.

Expected output ends with: `DOCTOR: ALL CHECKS PASSED — node is ready to start`

## Run

```bash
./target/release/xentrium --config config.toml
```

CLI overrides (all optional): `-n/--node-id`, `-a/--address`, `-p/--port`,
`-d/--data-dir`, `-b/--bootstrap`, `-l/--log-level`, `-c/--config`.

## Data & Logs

Per node, under `data_dir`:

| Path | Contents |
|------|----------|
| `node.identity` | Ed25519 node private key (hex) — **protect this file** |
| `tls/tls_credentials.bin` | TLS certificate + private key (CN = node_id) |
| `xentrium.db` (+ WAL) | SQLite state: validators, committee, peers, snapshots |
| `xentrium.log` | Structured log (if `logging.file` set) |

## Firewall

- **node1**: allow inbound TCP `30303` in the OS firewall and any external
  firewall/security group, if applicable. Outbound TCP must be open (default).
- **node2/node3**: no inbound rules needed — they connect outbound to node1.
  If you want direct peer connections, forward the node's port on the router.

## Graceful Shutdown

`CTRL+C` (Windows/Linux/macOS) triggers graceful shutdown: running flag
cleared, connections closed, DB connections released. State is persisted
continuously, so restart is safe at any time.

## Updating

1. `CTRL+C` the node, wait for `Node shutdown complete`.
2. Pull/build the new binary.
3. Restart with the same `config.toml` and `data_dir` — identity, TLS certs,
   and SQLite state are reused automatically.
