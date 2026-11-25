# Xentrium Open Testnet Readiness Report

**Date:** 2026-11-24
**Question:** is the network ready to accept wider public participation?
**Method:** subsystem-by-subsystem assessment against the evidence gathered
in Phase-4 Steps 4–7 (live 3-node operation, reboot drills, scale sims to
200 validators, abuse-protection implementation).

---

## Readiness Matrix

| Subsystem | Verdict | Evidence | Gaps |
|---|---|---|---|
| **Networking** | **PARTIALLY READY** | TLS+mTLS on all links; handshake auth with node_id binding; diff-serving gossip; per-IP conn caps, handshake windows, temp bans; survives reboots + reconnects on all 3 machines; 200-validator propagation ≤ ~11 s in sim | Single bootstrap (`178.18.248.9:443`) is a SPOF for *joining* (existing mesh survives its loss, new joins don't); outbound dialling unlimited; no NAT traversal story — NAT'd nodes can only dial out |
| **Security** | **READY** | Mutual TLS everywhere; signed envelopes; nonce+timestamp replay guard; TOFU cert pinning; message quotas on the 4 abuse-sensitive types; strike-ladder bans; abuse counters exposed; cert rotation + verification tooling; `--doctor` pre-flight | No security audit by third party; allowlist (`allowed_node_ids`) exists but is empty by design for open testnet — that's the point of this phase |
| **Consensus** | **PARTIALLY READY** | Autonomous validator registration, committee formation (deterministic, fleet-capped at 51), rotation, snapshot production — all verified live and at 200-validator scale | Snapshot/state machinery only — no block production, finality, or fork handling yet; committee cap must be identical fleet-wide (config, not consensus-enforced); a malicious-but-validly-signed validator can still register — admission is permissive |
| **Persistence** | **READY** | SQLite repositories for validators/committee/snapshots/peers; snapshot retention on *both* produce and receive paths (verified live: 49→21 rows); bounded advertised-snapshot vecs; DB stays ≤ ~86 KB at 200 validators | No backup/checkpoint export tooling yet; single-file SQLite is fine at this scale but is a future migration item |
| **Operations** | **PARTIALLY READY** | Auto-restart on reboot (systemd + scheduled tasks, all 3 verified); supervisor crash-loop recovery demonstrated; `/metrics` on every node incl. abuse + cert-expiry gauges; `--doctor` pre-flight; `--rotate-tls` runbook; health timers kill wedged nodes | Metrics are pull-only — no alerting pipeline wired; log timestamps mix UTC (node log) and local (supervisor log), already caused one debugging confusion; no central log aggregation |

## Summary Verdict

**PARTIALLY READY for open testnet** — the remaining blockers are
operational, not safety-critical:

1. **Bootstrap redundancy** — add a second bootstrap endpoint (any
   permanently-online mesh member) so joins don't depend on node1.
2. **Alerting** — `/metrics` exists; nothing watches it yet. Even a
   cron+threshold script on `peer_bans`, `replay_rejections`, and
   `cert_expiry_days` closes the loop.
3. **Admission policy** — decide whether open testnet keeps permissive
   validator admission (recommended for phase goals) and document it.

None of these block a *limited* open testnet (public bootstrap address,
documented join procedure). They block calling it production-grade.

## What an Attacker Can Still Do (known-accepted)

- Sybil-register many validators (registration is permissionless by design;
  committee cap + deterministic rotation limits blast radius to churn).
- Force connection churn within per-IP limits (3 conns, 20 handshakes/min
  — bounded cost).
- Replay old envelopes (rejected + counted; costs them, not us — feeds bans).
- Spam non-protected message types at `msg_rate_per_min` ceiling (dropped
  connection + strike on violation).

## Exit Criteria for "READY"

- ≥2 bootstrap endpoints live in deploy configs
- Metrics alerting or at minimum a dashboard consumer exists
- A second operator successfully joins a *new* node to the testnet using
  only public documentation
- `test_tcp_reconnect` fixed to use an ephemeral port (test hygiene)
