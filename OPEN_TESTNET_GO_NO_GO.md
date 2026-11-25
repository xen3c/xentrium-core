# Xentrium Open Testnet — GO / NO-GO Decision

**Date:** 2026-11-25
**Deciding:** readiness of the current architecture for public expansion,
classified per network size. Evidence: `EXPANDED_TESTNET_REPORT.md`,
`OPEN_TESTNET_HARDENING_REPORT.md`, live 3-node operation since Phase-3.

---

## Verdict by Network Size

| Size | Verdict | Basis |
|---|---|---|
| **10 nodes** | **READY** | Full convergence: propagation ~33 s, committee ~25 s, snapshots <1 s, faults heal in <30 s |
| **25 nodes** | **READY** | Same convergence profile (~31 s propagation); flat memory; traffic linear |
| **50 nodes** | **READY WITH RESTRICTIONS** | Converges fully but propagation stretches to ~97 s and join latency climbs to ~10 s/node — acceptable for a testnet, not for finality-sensitive use |
| **100 nodes** | **NOT READY** | Convergence did not complete within a ~45 min window under load; synchronous SQLite writes on the receive path (≈99.6k slow committee inserts) are the proven first bottleneck |

## Overall Decision

**GO for an open public testnet up to ~50 participating nodes.**
**NO-GO beyond that until the two persistence fixes land.**

This matches reality: the live network is 3 nodes, public participation
will grow gradually, and the 50-node headroom buys time to ship the
persistence batching + write-behind changes before demand arrives.

## Restrictions for the 50-node window

- Keep `committee_max_size = 51` and identical fleet-wide (already deployed)
- Keep the abuse-protection TOML settings as shipped (conn caps, handshake
  windows, bans, quotas)
- Announce open-testnet capacity as "up to ~50 nodes" publicly; publish
  the join procedure with `--doctor` as the pre-flight gate
- Add a second bootstrap endpoint before opening registration
  (single-bootstrap join SPOF is the one remaining operational hole —
  heals in ~29 s on restart, but new joins fail while it's down)

## Conditions to lift the 100-node NO-GO

| # | Change | Expected effect |
|---|---|---|
| 1 | Batch `committee_members` INSERTs into one transaction | Removes the measured write amplification — direct fix for the observed bottleneck |
| 2 | Async write-behind persistence queue | Receive path never blocks on disk again; propagation should return to ~linear |
| 3 | Re-run `expanded.rs` ramp to 100 | Acceptance: propagation converges < 5 min, committee < 2 min |

Optional but recommended: second bootstrap + peer exchange (de-concentrates
the star relay), SQLite WAL mode.

## What "GO" does NOT mean

- Not a production/finality claim — there is no block production or
  consensus finality in scope; this is network-layer readiness.
- Not a security audit — hardening is best-effort defensive
  (rate limits, bans, replay guard, mTLS), not formally reviewed.
- Validator admission remains permissive by design — sybil registrations
  are possible; committee cap + deterministic rotation bound the impact.

## Sign-off summary

- 10 nodes: **READY**
- 25 nodes: **READY**
- 50 nodes: **READY WITH RESTRICTIONS** (degrading propagation/join latency; capacity messaging required)
- 100 nodes: **NOT READY** (persistence-path bottleneck, proven and quantified)
