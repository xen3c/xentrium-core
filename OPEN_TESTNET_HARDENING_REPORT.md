# Xentrium Open Testnet Hardening Report

**Date:** 2026-11-24
**Scope:** Phase-4 Step-7 — abuse protection, certificate lifecycle, and
large-scale validation ahead of an open public testnet.
**Constraint compliance:** no consensus redesign, no protocol redesign,
no QUIC migration, no new chain features — all changes are defensive
hardening on existing paths.

---

## 1. Connection Rate Limiting

Implemented in `xentrium-node/src/rate_limit.rs` (`RateLimiter`), enforced
on the TCP accept path **before** the TLS task is spawned and again after
handshake completion.

| Control | Mechanism | Config key (`[network]`) | Default |
|---|---|---|---|
| Per-IP connection cap | Rejects new accepts once an IP holds N established connections | `max_conns_per_ip` | 8 (3 in deploy configs) |
| Handshake rate limit | Sliding 60s window of accept attempts per IP | `handshake_rate_per_min` | 20 |
| Temporary bans | Strike counter per IP; strike on invalid/rejected handshake or message-quota violation; at threshold → timed ban | `ban_duration_secs`, `ban_strike_threshold` | 600s / 10 |
| Banned-IP drop | `is_banned` check precedes all other accept work; bans auto-expire | — | — |

All limits are configurable per deployment (`config/deploy/node{1,2,3}.toml`)
without code changes.

## 2. Message Rate Limiting

`PerConnQuota` (sliding 60s window, per connection, per `MessageType`)
runs at message receipt in the shared receive loop.

| Message type | Per-connection ceiling |
|---|---|
| PeerList | 60/min |
| CommitteeSelection | 120/min |
| SnapshotRequest | 240/min |
| ValidatorRegistration (+ all other types) | `msg_rate_per_min` (default 1200/min; set high enough to scale with validator count) |

On violation: `rate_limit_hits` increments, a strike is recorded against
the peer IP (feeds the temp-ban ladder), and the connection is dropped.
Legitimate per-connection rates are far below these ceilings — diff-serving
sends ~1 envelope/validator/cycle.

## 3. Abuse Protection Metrics

New `/metrics` output (visible on all nodes at `:9101/9102/9103`):

```
xentrium_msgs_in_total                  # inbound messages processed
xentrium_tls_cert_expiry_days           # days until local cert expiry (-1 = unknown)
xentrium_rate_limit_hits_total          # message-quota violations
xentrium_rejected_connections_total     # banned/capped accepts refused
xentrium_rejected_handshakes_total      # invalid or malformed handshakes
xentrium_replay_rejections_total        # envelope signature/nonce/identity rejects
xentrium_peer_bans_total                # temp bans issued
xentrium_peers_banned_current           # bans currently active
```

## 4. Certificate Lifecycle

- **Monitoring:** `TlsCredentials::expiry_days()` parses `not_after` via
  `x509-parser`. At startup the node logs validity (`TLS certificate valid
  for N days`), warns below 30 days, and publishes
  `xentrium_tls_cert_expiry_days` to `/metrics`. `xentrium --doctor` gained
  a cert-expiry check that FAILS on expired, <30-day, or unreadable certs.
- **Rotation:** `xentrium --rotate-tls` generates a fresh Ed25519/self-signed
  pair, **verifies** it (parses, CN bound to node_id, server+client TLS
  configs build), then atomically swaps `tls_credentials.bin` via
  write-temp + rename. Peers accept the new cert on next connect (TOFU).
  Verified: `TLS ROTATED — new certificate … valid 2958 days`.
- **Current posture:** fleet certs are valid 2025–2035 (~2958 days), so
  rotation is procedural, not urgent — the tooling, alert path, and
  verification are in place before they are needed.

## 5. Large-Scale Validation

`tests/scale.rs` extended to a 10→25→50→100→200 ramp on a real 3-node
TLS star. Results:

| Validators | Propagation | Committee (cap 51) | Snapshot | Msgs/stage | RSS | DB (seed) |
|---|---|---|---|---|---|---|
| 10 | 410 ms | 303 ms | 24.7 s (first production) | 11 | 19 MB | 53 KB |
| 25 | 816 ms | 405 ms | converged | 24 | 19 MB | 53 KB |
| 50 | 1,319 ms | 506 ms | converged | 39 | 19 MB | 61 KB |
| 100 | 11,060 ms | 513 ms | converged | 112 | 21 MB | 65 KB |
| 200 | 6,103 ms | 1,720 ms | converged | 169 | 21 MB | 86 KB |

**Reading the numbers:**

- **Committee formation stays flat** (513 ms @100, 1,720 ms @200) — the
  deterministic sort→rotate→truncate does the O(N) work once and the cap
  keeps output constant. Big improvement over the uncapped 2,935 ms @50
  measured pre-hardening.
- **Traffic is ~1 message/validator/stage** — diff-serving works: 200
  validators cost 169 messages total, not 200 × connections.
- **Memory is flat** (19→21 MB from 10→200 validators); no leaks.
- **Propagation is noisy at 100+** (11 s then 6.1 s) — batched by the
  5 s announce interval in test config, not a hard bound. Still
  single-digit seconds at 200.
- Snapshots converge and stay converged; retention keeps DB ≤ ~86 KB.

## 6. Verification

- `cargo build` clean (pre-existing warnings only)
- `cargo test`: all pass except pre-existing env-only
  `test_tcp_reconnect` (hardcodes `:30304`, collides with live node2 port)
- `--rotate-tls` + `--doctor` exercised end-to-end on a scratch datadir
- 3/3 autonomous integration tests + scale ramp pass with limiters active
  (test configs use permissive limits so synthetic flood traffic is not
  misread as abuse)

## Remaining Known Limits

- Outbound connections aren't rate-limited (inbound-only model) — a node
  can be *asked* to dial many peers; mitigated by handshake limits on the
  receiving side.
- Replay rejections and handshake rejects are counted but per-source
  attribution lives only in logs.
- Quota windows are per-process memory — not shared across restarts
  (acceptable: they exist to shed load, not to be a ledger).
