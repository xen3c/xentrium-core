# Operational Observations — Live 3-Node Testnet

**Date:** 2026-11-23 (UTC)
**Window:** ~11.5 h continuous operation
**Build:** commit `7ff5b19` (accept-loop + buffered-receive + reconnect fixes)

Companion to `LIVE_TESTNET_REPORT.md`. This file records operational
lessons and watch-items for anyone running the nodes day to day.

---

## 1. What "healthy" looks like

Per-node steady-state log, every 30 s:

```text
Heartbeat: sent Ping to <peer> (id=N)
Heartbeat: 1 active connections          # 2 on the seed with both leaves up
Heartbeat: Pong from <peer>, latency XXXms
Peer discovery: N known peers            # every 60 s
Gossip maintenance: cleaned up expired messages
Snapshot advertisement: 0 available snapshots   # every 5 min
```

Healthy latency observed: **160–410 ms** between home NAT and the seed.

- `Heartbeat: N active connections` = **live links** — the real liveness signal.
- `Peer discovery: N known peers` = cumulative entries, **includes stale
  ones** — do not use it as a health check (see B-1 in the report).

## 2. Startup sequence (expected order)

```text
Starting Xentrium node: <name>
Network address: <bind>
Data directory: <dir>
Connected to bootstrap peer: <seed>      # leaf nodes only
Node operational
Verified handshake from <peer>: node_id=xnt-* (TLS-bound)
Starting heartbeat loop / gossip / peer discovery / snapshot loops
```

If `Failed to create log file` panics on start: the `data_dir` from the
config doesn't exist — `mkdir -p data/nodeN` first.

If `Network address`/`node_id` look like defaults instead of the config:
the `--config` path was wrong — the loader silently falls back to
defaults instead of erroring. **Always confirm the first log lines
reflect the intended config.**

## 3. Reconnect behaviour to expect

- Leaf re-dials the seed every **30 s** when the link is missing
  (`Reconnected to bootstrap peer` + `Verified handshake`).
- Seed-side restart = worst observed leaf outage **~45 s** (restart time
  + one reconnect tick). No operator action needed.
- Leaf process restart reconnects in **<1 s**.
- Every reconnect gets a **new source port** — the seed sees it as a new
  peer; the old entry is swept by heartbeat-miss cleanup within ~2–3 min.

## 4. Detecting a dead peer

- Clean close → `Removing failed connection` ~instantly.
- Silent death (power loss, kill, NAT mapping lost) →
  `Heartbeat: removing unhealthy peer` after ~2–3 min of missed pongs.
- A peer that stays `Unhealthy` in the tracker but whose connection is
  already gone is cleaned on the miss path, not the close path.

## 5. Known noise in logs

- rustls `TRACE`/`DEBUG` dumps (ClientHello, `PresharedKeyIdentity`)
  appear in the log file even at `info` level — large but harmless.
- `Snapshot advertisement: 0 available snapshots` is normal; no state
  has been produced yet.
- sqlx `DEBUG` query lines accompany every start — normal.
- "24h interval" log line actually fires every 5 min — cosmetic.

## 6. Watch-items / operational gaps

1. **Seed is a single point of failure** — leaves can't reach each other
   at all (star topology; leaf↔leaf links don't exist and can't form
   through NAT). If node1 dies, the network is down until it returns;
   leaves do reconnect automatically.
2. **`known peers` inflates monotonically** — discovery entries are not
   removed on socket-close, only on heartbeat-miss. Long-running seeds
   will accumulate stale entries (B-1). Cosmetic for now; fix before it
   matters.
3. **No validator/committee/snapshot activity** — these paths are
   implemented but inert. Nothing in normal operation registers a
   validator, forms a committee, or produces a snapshot. A follow-up
   phase must seed at least one validator to observe convergence.
4. **Port 443 usage** — node1 currently shares the HTTPS port. Fine for
   observation; conflicts with any real web service on that host.
5. **tmux is the process supervisor** — node1 does not restart on host
   reboot. A systemd unit is the proper fix.
6. **Config path typos are silent** — a wrong `--config` runs defaults
   rather than failing. Check startup log lines.
7. **Log dirs must exist** — create `data/nodeN` before first start or
   the node panics opening the log file.

## 7. One-liner health checks

```bash
# seed: live links + recent heartbeats
tail -20 data/node1/xentrium.log | grep -E "Heartbeat|Accepted|Verified"

# any node: is my link up?
grep -c "Verified handshake" data/nodeN/xentrium.log   # >=1

# established sockets to the seed (on the seed host)
ss -tn | grep 443
```
