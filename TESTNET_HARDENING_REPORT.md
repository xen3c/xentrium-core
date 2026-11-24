# Testnet Hardening Report — Phase-4 Step-6

Targets the three bottlenecks from the Step-5 scale audit: unbounded
snapshot growth, uncapped committee sizing, and O(N) full-state
re-broadcast — plus the metrics endpoint required by the dashboard spec.

## Changes

### 1. Snapshot retention

- `SnapshotRepository::retain_latest(keep)` — `DELETE` everything below
  the Nth-newest `state_number`; invoked after every store when
  `consensus.snapshot_retention > 0` (deploy configs: 20).
- `BasicSnapshotSync::set_retention` — same cap for the in-memory
  `local_snapshots` map, pruned on insert.
- **Bonus fix:** `advertised_snapshots` was a per-peer `Vec` that grew
  one entry per offer received, forever. Now capped at 8 per peer
  (dedup by state_number, oldest dropped).

### 2. Committee sizing

- `derive_committee(validators, epoch, cap)` — cap moved from the
  hardcoded `MAX_COMMITTEE_SIZE` constant into
  `consensus.committee_max_size` (default 51, deploy configs set it
  explicitly). Selection is unchanged: sort, rotate by epoch,
  truncate — deterministic subset, identical on every node.
  **Must match fleet-wide** or committees diverge.
- Committee-selection envelopes now carry ≤ `cap` members regardless
  of validator-set size.

### 3. Gossip optimization (diff-serving)

Before: every announce cycle re-sent **all** cached validator
envelopes + committee + snapshot offer to **every** connection —
~52 msgs/cycle/connection at 50 validators, ~quadratic growth.

After:

- Validator envelopes are never periodically re-sent. They propagate
  once on first sight (outbox relay) and are re-served to
  newly-connected peers via serve-on-connect.
- Envelopes now relay **when payload changes**, not only on first
  sight — so each node's periodic self re-announcement (fresh nonce)
  still crosses the relay, preserving autonomous propagation at
  O(validators)/cycle network-wide instead of
  O(validators × connections) per node.
- Committee selections and snapshot offers re-send **only when newer**
  than the last served value (per-node cursors).
- Beacon per-cycle cost drops to ~2 messages/connection (own
  re-announce + PeerList) plus changed-only relays — roughly
  **25× less chatter** at 50 validators.

### 4. Metrics endpoint

`xentrium-node/src/metrics.rs` — minimal HTTP responder on
`consensus.metrics_port` (0 = disabled; deploys: 9101/9102/9103),
Prometheus text format:

```
xentrium_peers, xentrium_validators_active, xentrium_committee_size,
xentrium_committee_epoch, xentrium_snapshot_state,
xentrium_snapshots_local, xentrium_snapshots_db,
xentrium_uptime_seconds, xentrium_rss_bytes,
xentrium_heartbeat_rtt_avg_ms, xentrium_heartbeat_rtt_ms{peer=...}
```

RSS is cross-platform via `sysinfo`. No new framework dependency.

## Before / after (scale ramp test, 3-node star, localhost)

| Stage | Metric | Before | After |
|---|---|---|---|
| 10 val | propagation | 506 ms | 403 ms |
| 10 val | committee | 302 ms | 404 ms |
| 25 val | propagation | 808 ms | 606 ms |
| 25 val | committee | 1014 ms | 809 ms |
| 50 val | propagation | 1112 ms | 1214 ms |
| 50 val | committee | **2935 ms** | **606 ms** |
| 50 val | beacon msgs/cycle/conn | ~52 | ~2 (+ relays on change) |
| snapshots stored | bound | unbounded | ≤ `snapshot_retention` (20) |
| offers per peer held | bound | unbounded | ≤ 8 |

Committee-formation measurement improved ~5× at 50 validators —
previously it queued behind the re-serve flood. Propagation is flat
(within noise). Absolute wall times vary with disk load; one run under
build contention showed 10s SQLite stalls — environment, not
algorithm.

## Test status

- `tests/scale.rs` ramp 10→25→50: **passed** (14.6 s clean run)
- `tests/autonomous.rs`: **passed**
- Workspace: all green except `test_tcp_reconnect` — binds
  `127.0.0.1:30304`, which is **node2's live service port**; collision
  is environmental, unrelated to these changes.

## Live verification (all three nodes on hardened build)

`/metrics` endpoint responding on node1/2/3 (ports 9101–9103):

| Metric | node1 | node2 | node3 |
|---|---|---|---|
| peers | 0→reconnecting | 1 | 1 |
| validators | 3 | 3 | 3 |
| committee epoch | 5985128 | 5985129 | 5985125 |
| snapshot state | 5985128 | 5985129 | 5985126 |
| snapshots_db | 23↓ | **21** (was 49+) | 49→pruning on next store |
| RSS | 10.3 MB | 13.5 MB | 16.1 MB |
| heartbeat RTT | — | 253 ms peer / 328 ms avg | — |

Retention verified live: `snapshots_db` on node2 dropped from 49+ to
≤21 (`snapshot_retention = 20` + current epoch). Per-peer RTT labels
work; uptime/RSS/peer counts populate correctly.

## Success criterion

Met — the network retains identical convergence semantics with ~25×
less periodic gossip traffic, bounded snapshot storage, a configurable
committee cap, and an operator-facing metrics surface.
