# Testnet Readiness Update

**Date:** 2026-11-16
**Phase:** PHASE-3 STEP-4 - Public Testnet Preparation
**Supersedes:** TESTNET_READINESS_REPORT.md (STEP-1 audit baseline)
**Protocol Version:** TAD v2.2.7 (Frozen)

---

## Classification Summary

| Domain | Status | Change from STEP-1 |
|--------|--------|--------------------|
| Networking | **Partially Ready** | Improved (heartbeat, schedulers) — TCP/no-TLS remains |
| Consensus | **Partially Ready** | Unchanged core logic; automation still missing |
| Persistence | **Ready** | Unchanged — file-backed durability verified |
| Operations | **Ready** | Improved (CLI, config, schedulers, shutdown, docs) |
| Security | **Not Ready** | Unchanged — no encryption/auth on P2P |

---

## 1. Networking — PARTIALLY READY

**Ready:**
- TCP transport with connect/accept, length-prefixed framing
- Peer discovery via bootstrap + PeerList gossip (multi-hop verified)
- Ping/Pong heartbeat with active pinging (STEP-3)
- Liveness tracking: last_ping/last_pong/latency, Healthy/Suspect/Unhealthy
- Automatic stale-peer removal and recovery
- Snapshot offer/request/data transfer with hash verification
- Background loops: accept, receive, heartbeat (30s), discovery (60s), gossip maintenance (30s), snapshot advertisement (5min)

**Not Ready:**
- TCP instead of QUIC (TAD deviation — deferred, ecosystem unstable)
- No TLS — all P2P traffic plaintext
- No certificate-based peer authentication
- Peer identity endpoint-derived, not cryptographic
- Scheduler intervals for discovery/gossip/snapshot hardcoded (only heartbeat is config-driven)
- No jitter on heartbeat pings

**Testnet verdict:** Functional for a trusted, small testnet. Acceptable with documented caveats.

---

## 2. Consensus — PARTIALLY READY

**Ready:**
- Bootstrap phase (0–20 validators) / Full Consensus phase (21+) detection
- Committee sizing `min(51, active)`, candidate pool `min(500, active)`
- Weighted random selection `sqrt(reputation)`, 5% weight cap
- 67% threshold vote aggregation
- Validator/committee state propagation and convergence (verified in tests)
- ValidatorManager: register, activate, suspend, committee select/rotate (manual trigger)

**Not Ready:**
- No automatic committee rotation scheduler (24h) wired to snapshots
- No automatic validator promotion/demotion lifecycle
- Slashing infrastructure exists but not operationally enforced
- Committee-only vote enforcement not integrated into message handling
- No real BFT consensus round (vote propose/commit messages not driven by block production — there is no block production yet)

**Testnet verdict:** Sufficient for bootstrap-phase testnet (≤20 validators) where committee coordination is informational. Full consensus requires Phase 3+ work.

---

## 3. Persistence — READY

**Ready:**
- File-based SQLite with WAL mode (`sqlite:{path}?mode=rwc`)
- Schema: accounts, transactions, validators, snapshots, governance_proposals, committee_members, peers
- Startup restoration of validators, committee, peers, snapshot metadata
- Verified by `persistence_audit.rs` — real process restart restores all state
- 6 durability tests + 1 audit test passing

**Limitations (non-blocking):**
- Snapshot payload restoration restores metadata; full payload path exists but replay not implemented
- No schema migration framework

**Testnet verdict:** Ready.

---

## 4. Operations — READY

**Ready:**
- `xentrium` CLI binary (`cargo run -p xentrium-node --bin xentrium`)
- `config.toml` loading with `[node]`, `[network]`, `[consensus]`, `[logging]` sections
- CLI overrides for node_id, address, port, data_dir, bootstrap, log_level
- Background schedulers: heartbeat, discovery, gossip maintenance, snapshot advertisement
- Graceful shutdown: CTRL+C / SIGINT / SIGTERM → clean `node.stop()`
- Structured logging via tracing (console + optional file)
- Deployment docs: TESTNET_DEPLOYMENT.md, TESTNET_CHECKLIST.md
- Testnet configs: config/testnet-node{1,2,3}.toml
- 3-node deployment verified by `testnet_deployment.rs`

**Limitations:**
- No systemd/init service files or Docker packaging
- No monitoring/metrics export
- Discovery/gossip/snapshot scheduler intervals not config-driven

**Testnet verdict:** Ready for manual multi-node deployment.

---

## 5. Security — NOT READY

**Not Ready:**
- No TLS/encryption on any P2P connection (TCP plaintext)
- No peer authentication (no certificates, no signature handshake)
- PeerID lacks public_key field (design deviation)
- No rate limiting on inbound connections
- Snapshot committee signatures not verified
- MachineID self-reported, unverified (Sybil risk)
- No replay protection on messages
- Wallet keys not yet integrated into node runtime (wallet lives in crypto crate only)

**Testnet verdict:** NOT READY for an untrusted public testnet. Acceptable only for an invite-only / trusted testnet where participants are known. Public deployment requires TLS + peer authentication at minimum.

---

## Progress vs STEP-1 Audit

| STEP-1 Gap | Status Now |
|------------|-----------|
| No CLI/binary | ✅ RESOLVED |
| No config file parsing | ✅ RESOLVED |
| No background schedulers | ✅ RESOLVED |
| No heartbeat mechanism | ✅ RESOLVED (active ping/pong/latency/health) |
| No signal handling | ✅ RESOLVED |
| TCP instead of QUIC | ⚠️ DEFERRED (documented) |
| No TLS | ❌ OPEN |
| No certificate auth | ❌ OPEN |
| No REST API | ❌ OPEN (out of scope this step) |
| No committee rotation automation | ❌ OPEN |
| No slashing enforcement | ❌ OPEN |

**Estimated completion vs STEP-1 baseline (~40%): ~60%**

---

## Recommendation

**Proceed to a TRUSTED/INVITE-ONLY testnet now** with the 3-node deployment verified by `testnet_deployment.rs` and documented in TESTNET_DEPLOYMENT.md. Do **not** open to the public internet until TLS + peer authentication are implemented (next phase).
