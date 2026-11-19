# First Testnet — Go-Live Sheet

## Ports

| Node | Label | Listen Port | Inbound Firewall Needed? |
|------|---------|-------------|--------------------------|
| 1 | node1 | **TCP 30303** | YES — open in node1 firewall + external security group |
| 2 | node2 | TCP 30304 | Optional (outbound-only works) |
| 3 | node3 | TCP 30305 | Optional (outbound-only works; NAT-safe) |

All inter-node traffic is **outbound-initiated**: node2/node3 dial node1;
node1 accepts. Once connected, traffic flows both ways over the same
mutually-authenticated TLS session — no port forwarding needed on node2/node3.

## Startup Order

1. **Node 1 (node1)** — seed node, no bootstrap peers. Start first, wait ~10 s.
2. **Node 2 (node2)** — `bootstrap_peers = ["<NODE1_IP>:30303"]`
3. **Node 3 (node3)** — `bootstrap_peers = ["<NODE1_IP>:30303"]`

Wait ~60 s after node3 for gossip convergence (PeerList announcements propagate
the full mesh).

## Commands

```bash
# Pre-flight (all nodes, must pass before launch)
xentrium --config <config.toml> --doctor

# Start
xentrium --config <config.toml>

# Stop
CTRL+C   # wait for "Node shutdown complete"
```

## Expected Log Messages

```
Starting Xentrium node: testnet-node-1
TCP transport started on 0.0.0.0:30303
TLS transport enabled (cert CN = xnt-<16hex>)
Node started successfully: testnet-node-1
Starting background networking tasks

# node2/node3 additionally:
Connecting to bootstrap peers: ["<NODE1_IP>:30303"]
Connected to bootstrap peer: <NODE1_IP>:30303

# every node, per peer:
Accepted connection from: <addr>            (inbound side)
Verified handshake from <addr>: node_id=xnt-... (TLS-bound)
Heartbeat: sent Ping to <addr> (id=N)
Heartbeat: Pong from <addr>, latency ...
```

### Green flags
- `Verified handshake ... (TLS-bound)` for each peer
- Repeating `Heartbeat`/`Pong` lines every ~30 s
- `Received and processed PeerList` / committee + validator log lines after gossip converges

### Red flags
- `Rejecting peer ... invalid handshake, cert/identity mismatch, not allowlisted, or replay`
- `TLS accept failed` / `TLS handshake failed`
- `Accept error` repeating (listener died)
- `Removing failed connection` in a loop (peer flapping)

## Validation Commands (runtime)

There is no RPC port; verification is via logs + a second doctor pass:

```bash
# Re-run doctor anytime (read-only checks; safe while node runs? — port check
# will FAIL while the node holds the port, which actually confirms it's listening)
xentrium --config <config.toml> --doctor
```

Note: while a node is running, doctor's `port availability` check will report
FAIL — this is expected and actually confirms the port is bound. All other
checks should still PASS.

Log inspection:

```bash
# Linux
tail -f ./data/node1/xentrium.log
# Windows
type .\data\node2\xentrium.log | more +0   # or open in editor
```

## Invite-Only Lockdown (after first successful run)

1. Collect `node_id=xnt-...` for all three nodes (doctor output / startup logs).
2. Set on every node:
   ```toml
   [network]
   allowed_node_ids = ["xnt-A", "xnt-B", "xnt-C"]
   ```
3. Restart all nodes. Verify `Verified handshake` still succeeds for the three
   and any new connector is dropped (`Rejecting peer ... not allowlisted`).

## Rollback Procedure

The testnet is stateless-config + per-node disk state; rollback is per-node:

1. `CTRL+C` the affected node; confirm `Node shutdown complete`.
2. **Config rollback**: restore previous `config.toml` (git or backup copy).
3. **State rollback** (only if state corruption suspected): stop node, move
   `data/nodeN/` aside (`mv data/node1 data/node1.bak`), restart —
   node regenerates identity + TLS creds + fresh DB. **Warning**: new identity
   means new `node_id`; update `allowed_node_ids` everywhere if lockdown is on.
4. **Binary rollback**: keep the previous `xentrium` binary; swap back and restart.
5. Full testnet rollback: stop node3 → node2 → node1 (reverse startup order),
   revert configs, restart in forward order.

## Go-Live Checklist

- [ ] Rust toolchain installed on all 3 nodes; `cargo build --release` done
- [ ] `<NODE1_PUBLIC_IP>` filled into node2/node3 configs
- [ ] `--doctor` ALL CHECKS PASSED on all 3
- [ ] node1 firewall allows inbound TCP 30303
- [ ] Startup order: node1 → node2 → node3
- [ ] `Verified handshake ... (TLS-bound)` seen for every peer
- [ ] Heartbeat/Pong repeating every 30 s on all nodes
- [ ] PeerList gossip converged (each node knows the other two)
- [ ] Validator + committee propagation log lines present, no rejections
- [ ] CTRL+C shutdown verified; restart resumes state
- [ ] (Recommended) `allowed_node_ids` lockdown applied and re-verified
