# PUBLIC TESTNET LAUNCH AUDIT — Phase 3 Step 9

Date: 2026-11-17
Scope: go/no-go assessment for invite-only public testnet launch
Method: synthesis of `TESTNET_READINESS_REPORT.md`, `PUBLIC_TESTNET_SECURITY_AUDIT.md`, `SECURITY_REMEDIATION_REPORT.md`, `TESTNET_READINESS_UPDATE.md`, plus verification of current code state. Audit only — no implementation.

## Executive Summary

**Recommendation: GO for invite-only public testnet — with restrictions.**

All Critical and High security findings from the Step-7 audit were remediated in Step-8 and verified by 11 dedicated integration tests (200/200 total passing). The node now provides encrypted mutual-TLS transport, pubkey-bound cryptographic identity, signed and replay-protected validator/committee propagation, signed peer gossip, peer allowlisting, and bounded message sizes.

The system is **not ready for an open public testnet** — remaining Medium items (rate limiting, unencrypted key-at-rest, no certificate rotation, self-declared machine IDs) become exploitable at scale without an allowlist gate.

## Re-evaluation by Domain

### Networking — READY WITH RESTRICTIONS
- TLS 1.3 mutual authentication on all node traffic; identity-bound certs (CN = pubkey-derived node_id)
- Signed handshake with 5-min freshness + nonce replay protection
- Peer allowlist (`allowed_node_ids`) available for invite-only deployment
- **Remaining**: no rate limiting (M1), TLS 1.2 still permitted (L2), no hostname verification (M4), 50ms accept-loop timeout can drop slow handshakes under load

### Persistence — READY
- File-backed SQLite (WAL) verified across full process restart: validators, committee, peers, snapshots all restore (`PERSISTENCE_AUDIT.md`, `DURABILITY_REPORT.md`)
- TLS credentials + node identity persisted atomically

### Recovery — READY
- Clean shutdown, restart recovery, crash-recovery infrastructure, multi-node rejoin all tested (25 multi-node tests over mutual TLS)

### Identity — READY
- Persistent Ed25519 identity; `node_id` now cryptographically bound to public key (C1 closed)
- **Remaining**: private key stored unencrypted on disk (M2)

### TLS — READY WITH RESTRICTIONS
- Mutual TLS, custom verifiers reject malformed/expired certs, cert CN cross-checked against signed handshake
- **Remaining**: TOFU trust model — no CA/pinning by default (mitigated by allowlist for invite-only), 10-year certs, no rotation/revocation (M3), `localhost` SAN only (M4)

### Replay Protection — READY
- Nonces on handshake/announcement/envelope; bounded ReplayGuard; ±5min timestamp window (H3/H4 closed)

### Validator Propagation — READY
- Signed domain-separated envelopes; signature + freshness + replay + connection-identity binding verified before processing; unsigned payloads rejected (C3 closed)

### Committee Propagation — READY
- Same signed-envelope treatment as validator registration; `update_committee` only applied after full verification

### Peer/Gossip Propagation — READY WITH RESTRICTIONS
- Signed announcements verified (signature, binding, freshness, replay, allowlist) before caching/relay (H2 closed)
- **Remaining**: gossiped `machine_id` still self-declared (M5); announcement relay is unweighted (any authenticated peer can relay any valid announcement — acceptable under allowlist)

## Critical/High Closure Verification

| Finding | Status | Evidence |
|---------|--------|----------|
| C1 node_id/pubkey binding | ✅ CLOSED | `verify_*` rejects mismatched IDs; `c1_node_id_must_match_public_key` test |
| C2 message size DoS | ✅ CLOSED | 16 MiB pre-allocation cap; `c2_oversized_frame_rejected` test |
| C3 unsigned validator/committee | ✅ CLOSED | Signed envelopes + connection binding; 3 dedicated tests |
| H1 TOFU/no pinning | ✅ CLOSED (via allowlist) | `allowed_node_ids` enforced; 2 dedicated tests |
| H2 unsigned gossip | ✅ CLOSED | Signed announcements end-to-end; 3 dedicated tests |
| H3 replay | ✅ CLOSED | ReplayGuard on all signed types |
| H4 timestamp window | ✅ CLOSED | ±5min enforced |

## Remaining Medium/Low Items

| ID | Item | Launch Impact |
|----|------|---------------|
| M1 | No rate limiting / connection quotas | Medium — invite-only bounds exposure; monitor |
| M2 | Identity key unencrypted at rest | Medium — host compromise = identity theft; document OPSEC |
| M3 | No cert rotation/revocation | Medium — manual re-key procedure needed |
| M4 | No hostname verification | Low-Med — IP-based testnet acceptable |
| M5 | machine_id self-declared | Low — not used for auth decisions |
| M6 | Unbounded-ish gossip tables | Low — TTL+capacity bounded |
| L1–L3 | Log topology, TLS 1.2, error verbosity | Low |

## Readiness Classification

| Deployment Model | Classification | Basis |
|------------------|----------------|-------|
| **Private Testnet** (trusted operators) | **READY** | All blockers closed; durability + ops verified |
| **Invite-only Public Testnet** | **READY WITH RESTRICTIONS** | Requires `allowed_node_ids` populated with participant identities; rate-limit monitoring; OPSEC for key material |
| **Open Public Testnet** | **NOT READY** | M1 rate limiting, M3 cert lifecycle, and economic Sybil barrier (TAD) still missing; unauthenticated-peer exposure untested at scale |

## Go / No-Go Recommendation

**GO — invite-only public testnet**, conditional on:

1. Deploy with `allowed_node_ids` populated for all launch participants (bootstrap set)
2. Publish node startup docs + `TESTNET_CHECKLIST.md` verification steps to participants
3. Operator OPSEC: protect `data/node.identity` and `data/tls/` (filesystem permissions, host hardening)
4. Monitoring: watch connection counts, handshake rejection logs, memory usage (M1/M6)

**NO-GO — open public testnet** until M1 (rate limiting), M3 (cert rotation), and TAD economic Sybil resistance are implemented.

## Remaining Blockers

**Before invite-only launch:** none technical — procedural items above only.

**Before open public launch:**
- Rate limiting / per-peer quotas (M1) — ~2–3 days
- Certificate rotation + revocation procedure (M3) — ~2–3 days
- Economic barrier for validator registration per TAD — ~1–2 weeks (consensus-adjacent, needs care)
- Load testing at 50–100+ node scale — ~1 week

## Estimated Time to Public Launch

- **Invite-only public testnet:** ready now (0 days — configuration + participant onboarding only)
- **Open public testnet:** ~3–4 weeks of focused work (rate limiting, cert lifecycle, Sybil economics, scale testing)

## Test Evidence

```
Total: 200/200 tests passing
xentrium-node: 75 tests incl. 11 security_remediation + 6 transport_security
xentrium-network: 93 tests (identity, TLS, transport, heartbeat, discovery, gossip, sync)
Prior audits: PERSISTENCE_AUDIT (durability), STABILITY_REPORT, NODE_OPERATION_REPORT,
              HEARTBEAT_REPORT, TESTNET_DEPLOYMENT — all verified
```
