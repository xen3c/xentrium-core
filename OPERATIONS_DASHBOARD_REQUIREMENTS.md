# Operations Dashboard Requirements — Phase-4 Step-5

Derived from the scale audit: the metrics below are what an operator
needs to watch the network's health and headroom. Grouped by source;
each notes how it can be collected today vs what an instrumentation
endpoint should expose.

## 1. Process health

| Metric | Why | Source today |
|---|---|---|
| Process alive / listener bound | dead node = silent network | health probes (`xentrium-health.sh` / `.ps1`) |
| RSS memory | leak detection | OS (`ps`, `Get-Process`) — no endpoint |
| CPU % | consensus/verification load | OS sampling — no endpoint |
| Uptime | restart-loop detection | supervisor log / journald |
| Log write freshness | wedged-event-loop detection | health probes already check this |

## 2. Network

| Metric | Why | Source today |
|---|---|---|
| Active connection count | partition detection | log (`Heartbeat: N active connections`) |
| Connected peer list | topology view | internal `connections` map — needs endpoint |
| Heartbeat RTT (min/avg/max per peer) | link degradation, latency SLOs | parsed from log lines |
| Bootstrap reconnect failures | seed unreachable | log (`Failed to reconnect`) |
| Messages in/out per type | envelope-chatter scaling (the O(N) beacon) | counted from logs; needs counters |

## 3. Consensus

| Metric | Why | Source today |
|---|---|---|
| Active validator count | the scaling dimension | `validator_manager.active_validator_count()` |
| Committee member count + membership hash | convergence check across nodes | `get_committee_members()` |
| Current committee epoch + leader | rotation health; who should produce snapshots | log + `state_number()` |
| Validator registration rate | join churn | log lines |
| Rejected envelopes (auth/replay) by reason | attack surface + sync delays | log — needs counters |

## 4. Synchronization & storage

| Metric | Why | Source today |
|---|---|---|
| Current snapshot state number | convergence height | `get_current_state_number()` |
| Snapshot age (now − latest epoch) | stalled leader detection | derived |
| Snapshot pull latency (offer→installed) | sync path health | log pair timing |
| DB file size + row counts (validators/committee/snapshots) | unbounded-growth detection (no pruning today) | filesystem + SQL |
| Snapshot table row count | retention headroom — currently unbounded | SQL `COUNT(*)` |
| Peers/announcements known | gossip propagation | `known_announcement_count()` |

## 5. Service layer

| Metric | Why |
|---|---|
| Service/task state (systemd active, scheduled-task last-run result) | auto-start actually armed |
| Restart count in last hour | crash-loop signal |
| Health-probe last result + consecutive failures | flapping detection |

## 6. Alert thresholds (suggested starting points)

| Condition | Severity |
|---|---|
| Node not listening / health probe fails twice | critical |
| Zero active connections for >2 min on a non-seed node | critical |
| Heartbeat RTT >2s sustained | warning |
| Validator count disagreement between nodes after 2 epochs | warning |
| Committee epoch advancing but snapshot state frozen >3 epochs | warning (stale-committee bug pattern) |
| Snapshot table >10k rows | warning (no pruning — needs retention policy) |
| Rejected-envelope rate spike | warning (possible replay/storm) |

## 7. Instrumentation gap (to build)

Today every metric above is scraped from logs or OS tools. A minimal
observability surface should be added:

- `GET /metrics` (Prometheus text format) on a configurable port —
  counters (messages in/out, rejections by reason), gauges
  (validators, committee epoch, snapshot state, connections), and
  histograms (heartbeat RTT, propagation latency).
- `GET /healthz` — 200 when running + listener bound + event loop
  fresh; gives systemd/Task-Scheduler probes a stronger signal than
  log-mtime checks.
- Structured JSON logging option so log scrapers stop parsing
  human-readable lines.

Priority order: metrics endpoint → healthz → alert wiring →
dashboard (Grafana or equivalent) once metrics exist.
