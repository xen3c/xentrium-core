# Xentrium Expanded Testnet Report

**Date:** 2026-11-25
**Scope:** Phase-4 Step-8 — validate operation beyond the 3-node deployment:
multi-node scale ramp (10→25→50→100 nodes) and fault injection.
**Method:** `xentrium-node/tests/expanded.rs` — real in-process nodes with
real TCP+TLS, signed envelopes, SQLite persistence. Star topology matching
the live testnet (all leaves bootstrap to node0). Every node self-registers,
so N nodes = N validators.

---

## 1. Scale Ramp — Measured Results

| Nodes | Peer discovery | Validator propagation | Committee convergence | Snapshot sync | Total messages | RSS (whole harness) |
|---|---|---|---|---|---|---|
| 10 | 5 ms | 32.9 s | 25.1 s | < 1 s (already converged) | 1,096 | 31 MB |
| 25 | 33 ms | 31.0 s | 11.2 s | < 1 s | 5,703 | 53 MB |
| 50 | 485 ms | 96.5 s | 25.7 s | < 1 s | 25,686 | 102 MB |
| 100 | — | **did not converge within ~45 min** | — | — | — | ~174 MB |

CPU: negligible (<5% of one core average) — the bottleneck is not compute.

**Read carefully:** RSS is the whole test process (~1–2 MB/node — memory is
not a limit). "Total messages" is cumulative inbound count across all nodes.

### Node join latency (a second scaling signal)

Time to spawn + connect + handshake + first sync, per node, as the
network grew:

| Node index | Spawn+join time |
|---|---|
| 3–24 | 50–130 ms |
| 25–39 | 0.2–11 s |
| 65–91 | 1.2–40 s (median ~11 s) |

Join latency grows superlinearly — by ~90 nodes a new member takes tens
of seconds to get through the accept/handshake/serve path at the star
center.

## 2. Fault Injection (8-node star)

| Scenario | Result |
|---|---|
| Leaf node crash (`stop()`) | Surviving mesh stayed committee-converged throughout |
| Node reboot (restart, same datadir) | Reconnected + full 8-validator state in **50 ms** (persisted state) |
| Delayed rejoin (down ~25 s, ~2 announce cycles) | Full validator + snapshot catch-up in **9.4 s** |
| Network partition (bootstrap down) | Leaves retained complete state (8/8 validators); no loss |
| Bootstrap node restart | Mesh re-formed + re-converged in **29.2 s** (bounded by reconnect retry interval) |

All fault scenarios pass: state convergence, committee convergence, and
snapshot convergence all hold after faults heal.

## 3. First Scaling Bottleneck — identified

**Synchronous SQLite writes on the message-processing path.**

Hard evidence from the 100-node run: **99,647 `INSERT INTO committee_members`
statements exceeded the 1 s slow-query threshold** (plus 7–9 s
`INSERT OR REPLACE INTO snapshots`). Committee persistence writes one row
per member per committee change, sequentially, on the same task that
processes inbound messages. At 100 nodes each rotation writes ~51 rows per
node — ~5,100 row inserts fleet-wide per epoch — and every message behind
a write stalls.

Secondary bottlenecks observed:

- **Star-center concentration**: all relay + serve traffic funnels through
  node0's single receive task; join rate at the tail (nodes 65–91) shows
  the accept path falling behind.
- **Per-check measurement cost**: polling 100 nodes' lock-guarded state
  under write stalls stretches convergence checks — consistent with a
  system that is *making progress slowly*, not deadlocked.

**Environment caveat (honest):** all 100 nodes shared one Windows temp
disk + antivirus scanning — much harsher than real deployments where each
node has its own disk. A real 100-node network would land between the
50-node and 100-node observations. But the per-row synchronous insert
pattern is a code-level issue independent of disk speed.

## 4. Quantified Upper Limit (current architecture)

| Metric | Value |
|---|---|
| Converges reliably | ≤ 50 nodes |
| Converges slowly (degrading) | ~50–75 nodes |
| First hard bottleneck | ~100 nodes — synchronous per-row SQLite writes serializing the receive path |
| Memory per node | ~1–2 MB (not a limit) |
| CPU | negligible at 100 nodes |

## 5. Recommended Next Changes (no redesign needed)

In priority order — all are incremental hardening, none are protocol or
consensus changes:

1. **Batch committee persistence** — single-transaction multi-row INSERT
   (or executemany) instead of per-row awaits. Removes ~99% of the write
   amplification directly.
2. **Write-behind persistence queue** — snapshot/committee/validator writes
   go on a bounded channel drained by a dedicated task; receive path never
   blocks on disk.
3. **SQLite WAL + busy_timeout** (if not already enabled) — removes
   lock-contention stalls between writer and metrics/status readers.
4. **Second bootstrap + peer exchange** — leaves form a partial mesh
   instead of a pure star; spreads relay load and removes the join
   concentration at node0 (also fixes the documented bootstrap SPOF).
5. **Test hygiene** — fix `test_tcp_reconnect` hardcoded `:30304`
   (collides with live node2 port).

None of these are required for the live 3-node testnet today; they are
required before marketing "100 nodes supported".
