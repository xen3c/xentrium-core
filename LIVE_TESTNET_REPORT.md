# Live Testnet Report — Phase 4, Step 1

**Date:** 2026-11-23 (UTC)
**Scope:** Observation-only pass over the live 3-node Xentrium network.
No protocol, consensus, or network-feature changes were made during
observation. (Two transport-layer robustness fixes — commit `4921645`
and `7ff5b19` — were deployed *before* this observation window and are
the build under test.)

---

## 1. Environment

| Role | Config | Connectivity |
|---|---|---|
| node1 | `config/deploy/node1.toml` | Permanent bootstrap/seed; public listener on TCP 443 |
| node2 | `config/deploy/node2.toml` | Always-on peer; outbound-only to node1 (behind NAT) |
| node3 | `config/deploy/node3.toml` | Intermittent peer; outbound-only to node1 (behind NAT) |

All nodes run the same release build (`cargo build --release -p xentrium-node`,
commit `7ff5b19`). node2 and node3 share the same public IPv4 (NAT); node1
distinguishes them by source port. Node identities are per-node Ed25519
keypairs bound to self-signed TLS certificates (`xnt-*`); identity values
are intentionally omitted from this report.

## 2. Verified Behaviour Summary

| Requirement | Result |
|---|---|
| Node startup | PASS |
| Node shutdown (graceful + killed) | PASS |
| Node rejoin | PASS |
| TLS reconnect | PASS |
| Heartbeat behavior | PASS |
| Peer removal (socket-close path) | PASS |
| Peer removal (heartbeat-miss path) | PASS |
| Peer recovery (automatic redial) | PASS |
| Validator convergence | NOT EXERCISED (no validators registered) |
| Committee convergence | NOT EXERCISED (empty committee) |
| Snapshot recovery | NOT EXERCISED (0 snapshots produced) |
| Leave → remove → rejoin → resync cycle | PASS (partial — see §7) |

## 3. Timeline of Observed Events (UTC)

| Time | Node | Event |
|---|---|---|
| 2026-11-22 23:45:14 | node1 | Accepted node2 (`Verified handshake`, TLS-bound) |
| 2026-11-22 23:54:46 | node1 | Accepted node3 (`Verified handshake`, TLS-bound) |
| 2026-11-23 00:35:06 | node1 | `Heartbeat: removing unhealthy peer` — node3 departed |
| 2026-11-23 03:37:52 | node1 | Accepted node2 on new public IP (home IP rotated) |
| 2026-11-23 03:40:12 | node1 | Removed node2's stale old-IP connection via heartbeat-miss |
| 2026-11-23 10:47:55 | node1 | Accepted node3 — automatic rejoin after host power-on |
| 2026-11-23 10:51:03 | node1 | `Removing failed connection` — node2 process killed |
| 2026-11-23 10:51:30 | node1 | Accepted node2 rejoin; handshake verified in ~60 ms |
| 2026-11-23 10:53:24 | node1 | Heartbeat-miss sweep removed node2's stale discovery entry |
| 2026-11-23 10:55:19 | node2 | Detected node1 shutdown (`UnexpectedEof`), connection removed |
| 2026-11-23 10:56:02 | node1/node2 | node1 restarted; node2 auto-reconnected + verified (~43 s outage) |
| 2026-11-23 10:57–10:58 | node1/node3 | node3 auto-reconnected + verified; `2 active connections` |

## 4. Scenario Details

### 4.1 Startup

Both leaf nodes (`node2`, `node3`) complete startup in <1 s:

```text
Starting Xentrium node: testnet-node-N
Network address: 0.0.0.0:3030N
Connected to bootstrap peer: <node1-ip>:443
Node operational
Verified handshake from <node1-ip>:443: node_id=xnt-<node1> (TLS-bound)
```

Observed connect → verified-handshake latency: ~60–250 ms over WAN.
node1 logs the mirror image (`Accepted connection` → `Verified handshake`).

### 4.2 Shutdown — graceful

When a leaf terminates cleanly (SIGINT), node1 logs
`Removing failed connection <peer>` on the next 50 ms receive poll —
effectively instantaneous.

### 4.3 Shutdown — killed / host powered off

When a peer dies without a clean TCP close (process kill, host power-off,
or NAT-mapping loss), no FIN/RST arrives. Detection then falls to the
heartbeat tracker:

- 1 missed pong → peer marked `Suspect`
- ≥2 misses (≈60–90 s at the 30 s interval) → `Unhealthy` →
  `Heartbeat: removing unhealthy peer <peer>`; connection and discovery
  entry removed.

Observed detection times: ~3 min (node3 power-off), consistent with the
30 s interval + miss counting + peer timeout. NAT/IP rotation of node2's
home link took ~2.5 min to sweep — same path.

### 4.4 Rejoin

Two rejoin paths were observed:

- **Automatic**: the per-node bootstrap reconnect task re-dials every
  30 s when the bootstrap connection is missing. node3 rejoined 10:47:55
  on host power-on with zero operator action; node2 rejoined 10:56:02
  after the node1 restart (≈43 s total outage including node1's downtime).
- **Manual restart**: process relaunch → connect + verify in ~250 ms.

Every rejoin re-runs the full path: TCP connect → TLS 1.3 handshake →
signed identity handshake → `Verified handshake … (TLS-bound)` on both
sides. Rustls session tickets appear in trace logs, but the signed
handshake verification runs on every (re)connect regardless.

### 4.5 Heartbeat behavior

- Ping/Pong every 30 s in both directions on every established link.
- Observed WAN latencies: ~160–410 ms (EMA tracked per peer).
- Missed-pong accounting and Suspect/Unhealthy transitions behave as
  designed (see §4.3).

### 4.6 Peer removal

Two independent removal paths confirmed:

| Trigger | Log signature | Latency |
|---|---|---|
| TCP close/error | `Removing failed connection <peer>` | ~instant (50 ms poll) |
| Heartbeat misses | `Heartbeat: removing unhealthy peer <peer>` | ~2–3 min |

Note: socket-close removal clears the `connections` map only; the
`discovery` entry lingers until the heartbeat-miss sweep (see §6, B-1).

### 4.7 Peer recovery

- Leaf → seed: automatic, via the 30 s bootstrap reconnect task.
- Seed → leaf: not applicable — leaves dial; node1 never initiates.
- Leaf ↔ leaf: none — node2 and node3 never connect directly (star
  topology through node1); each reports `1 known peer` (node1) while
  node1 reports `2 active connections` plus stale discovery entries.

### 4.8 Validator convergence

Not exercised. No validators are registered on any node; the validator
manager initializes empty on every start. The signed
`ValidatorRegistration` broadcast path exists (`broadcast_validator_registration`)
but nothing calls it during normal runtime — there is no registration CLI
or automatic self-registration.

### 4.9 Committee convergence

Not exercised. Committee state loads empty from storage
(`Loaded committee` never fires; `committee_members` table empty).
The signed `CommitteeSelection` broadcast path exists but is unused —
there is no committee production in the current testnet flow.

### 4.10 Snapshot recovery

Not exercised. `Snapshot advertisement: 0 available snapshots` every
5 min on all nodes. No `SnapshotOffer`/`SnapshotRequest`/`SnapshotData`
messages were observed — no snapshots are being produced, so there is
nothing to request or recover. Fresh node state is trivially "in sync"
(empty accounts/transactions tables).

## 5. Required Validation: Leave → Remove → Rejoin → Rediscover → Resync

```text
node3 offline          →  heartbeat misses on node1 (~60–90 s)
peer removed           →  `Heartbeat: removing unhealthy peer` + conn drop
node3 returns          →  bootstrap reconnect task dials node1 (≤30 s)
peer rediscovered      →  TLS + signed handshake re-verified (TLS-bound)
state resynchronized   →  identity re-verified, announcements re-exchanged;
                          no snapshot data required (empty state)
```

Result: **PASS** for connectivity/identity resync. State resync beyond
identity/announcements is unexercised because no chain state exists yet
(§4.10).

## 6. Findings Register

### Bugs

| ID | Severity | Description |
|---|---|---|
| B-1 | Low | `Removing failed connection` clears `connections` but not the `discovery` entry; stale peers accumulate (`Peer discovery: 5 known peers` vs `2 active connections`) until the heartbeat-miss sweep — and discovery removal matches by tracker `peer-{addr}` identity, which may never match the stored peer record |
| B-2 | Low | Connection removal on heartbeat-miss matches by **port only** (`a.port() == peer_id.endpoint.port()`); two NAT'd peers can collide |
| B-3 | Info | Snapshot advertisement log says "24h interval" but the loop actually fires every 300 s |

### Unexpected behaviour (explained, not fixed)

| ID | Description |
|---|---|
| U-1 | Each reconnect allocates a new source port, so node1 accumulates one discovery entry per rejoin — counts inflate monotonically across the observation window (root cause of B-1) |
| U-2 | `Peer discovery: N known peers` counts stale entries, not live links; `Heartbeat: N active connections` is the accurate liveness signal |
| U-3 | Home public IP rotation mid-session leaves a ghost connection on node1 until heartbeat-miss cleanup (~2.5 min) — expected TCP behaviour, no RST possible |

### Expected behaviour (verified)

| ID | Description |
|---|---|
| E-1 | Every connect/reconnect performs TLS 1.3 + signed Ed25519 handshake; `Verified handshake … (TLS-bound)` logged on both sides |
| E-2 | Heartbeats 30 s bidirectional, latency ~160–410 ms, EMA tracked |
| E-3 | Instant removal on socket close; heartbeat-miss removal ~2–3 min |
| E-4 | Bootstrap auto-reconnect restores links in ≤30 s + verification |
| E-5 | Node identity persists across restarts (same `xnt-*` id re-verifies) |

### Improvements (for later phases)

| ID | Description |
|---|---|
| I-1 | Sync `discovery` on socket-close removal (fix B-1); make removal match full `SocketAddr`, not port only (B-2) |
| I-2 | Implement real peer-list gossip — `receive_peer_list(vec![])` is a stub; leaves never learn about each other |
| I-3 | Suppress rustls TRACE dumps (ClientHello/PresharedKeyIdentity) from the node log at `info` level |
| I-4 | Align snapshot-advertisement interval/log wording (300 s vs "24h") |
| I-5 | Exercise validator/committee paths: seed ≥1 validator registration + committee so convergence and snapshot sync can be observed |
| I-6 | Add an option for seed-initiated peer introduction so leaf↔leaf links can form behind NAT |

## 7. Success Criteria

> "The live network remains healthy while nodes intermittently leave
> and rejoin."

**Met.** Across the window the network absorbed: host power-off, process
kill, public-IP rotation, and a full seed restart — and recovered every
time without manual network intervention. node1 ended the window with
`2 active connections`, both heartbeating at normal latency.
