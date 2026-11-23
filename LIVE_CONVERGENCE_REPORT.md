# Live Convergence Report — Phase 4, Step 2

**Date:** 2026-11-23 (UTC)
**Build:** commit `7ff5b19` — observation only, no protocol/consensus changes.
**Subject:** committee convergence and snapshot synchronization across
the live 3-node testnet, driven by `examples/testnet_admin.rs`.

---

## 1. Committee formation & convergence — VERIFIED

A 1-member committee `{test_validator}` at state 1 was broadcast as a
signed envelope (`xnt-committee-selection` domain). Convergence evidence:

| Node | Log | Result |
|---|---|---|
| node1 | `Updated committee with 1 members for state 1` | converged |
| node2 | `Updated committee with 1 members for state 1` | converged |
| node3 | `Updated committee with 1 members for state 1` | converged |

All receivers verified signature, replay nonce, and handshake-identity
binding before applying — **convergence by signed input**. No committee
voting/rotation occurred (there is no automatic rotation trigger at
runtime despite the `auto_committee_rotation` config flag — nothing
invokes it).

## 2. Snapshot advertisement — VERIFIED

- Before activation: `Snapshot advertisement: 0 available snapshots`
  on every node, every 5 min.
- After `SnapshotData` was received and stored, the receiving node's
  next tick reported **`Snapshot advertisement: 1 available snapshots`**
  (observed on node2 at 20:11:30; node1/node3 store via the same
  `receive_snapshot` path).
- Note: the advertisement loop logs counts but does not itself send
  `SnapshotOffer` messages — offers only move when explicitly broadcast.

## 3. Snapshot transfer — VERIFIED (push path)

```text
Sender:   broadcast_snapshot_offer -> broadcast_snapshot_data
node1:    Received newer snapshot offer: state 1 → Received and stored snapshot
node2:    Received newer snapshot offer: state 1 → Received and stored snapshot
node3:    Received newer snapshot offer: state 1 → Received and stored snapshot
```

`receive_snapshot` verifies `snapshot_hash == state:hash:timestamp`
before storing — a malformed hash would be rejected.

## 4. Snapshot installation — VERIFIED

On every receiver, `receive_snapshot` stored the snapshot and advanced
`current_state_number` to 1. Confirmed externally by the advertisement
count going 0 → 1, and on the driver node by the `check` dump:
`STATE snapshots: 1 available, current_state=1`.

## 5. Snapshot recovery — PARTIALLY VERIFIED

| Path | Result |
|---|---|
| Storage reload (broadcasting node) | VERIFIED — restart loads latest snapshot from SQLite (`Loaded snapshot from storage: state 1`, real `get_latest()` path) |
| Pull protocol (`SnapshotRequest`) | NOT OBSERVED — receivers log `Received newer snapshot offer` but never send a request; the offer handler is log-only. Transfer only happened because the sender pushed `SnapshotData` |
| Network recovery on receivers | PARTIALLY — after `node3` restart wiped in-memory snapshots, re-broadcast of `SnapshotData` reinstalled state 1 (~1 s). No node auto-requests missing snapshots |

## 6. Restart behaviour (convergence view)

| Node | Restart effect | Recovery |
|---|---|---|
| node2 (validator node) | validator/committee/snapshot wiped (memory-only receiver state) | re-broadcast → full state in ~1 s; auto-reconnect to node1 |
| node3 (non-validator) | identical wipe | identical recovery at 21:53:15 |
| driver (state origin) | full SQLite reload | autonomous — validator, committee, snapshot all restored |

## 7. Classification

| Area | Classification |
|---|---|
| Validators | PARTIALLY VERIFIED — propagation/convergence verified; persistence & recovery only on the origin node and only for the hardcoded `test_validator` address |
| Committee | VERIFIED for propagation/convergence; PARTIALLY VERIFIED for persistence/recovery; committee voting NOT OBSERVED |
| Snapshots | PARTIALLY VERIFIED — offer/transfer/install/reload verified on push path; `SnapshotRequest` pull path and auto-recovery NOT OBSERVED |

## 8. Key gaps for later phases

| ID | Finding |
|---|---|
| C-1 | `SnapshotOffer` never triggers `SnapshotRequest` — the pull/recovery half of snapshot sync is dead code at runtime |
| C-2 | Receiver-side validator/committee/snapshot state is volatile — no persistence on receipt |
| C-3 | No automatic validator registration, committee rotation, or snapshot production — all activation is external today |
| C-4 | Broadcasts are direct-connection only; convergence cannot propagate through the seed to indirectly connected peers |
| C-5 | `auto_committee_rotation` config flag is inert — no runtime rotation |

**Overall:** committee and snapshot *state* were demonstrably created,
propagated, stored, and re-converged on the live network. The missing
half is autonomous behaviour — nothing produces, requests, or re-serves
state without an external driver.
