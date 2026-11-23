# Live Validator Report — Phase 4, Step 2

**Date:** 2026-11-23 (UTC)
**Build:** commit `7ff5b19` — no protocol, consensus, or architecture
changes made during this step.
**Tooling:** `xentrium-node/examples/testnet_admin.rs` — a driver that
joins the testnet as a regular node (`testnet-admin`) and exercises the
existing public `Node` API (`broadcast_validator_registration`,
`broadcast_committee_selection`, `broadcast_snapshot_offer`,
`broadcast_snapshot_data`, `persist_*`). Broadcasts reach directly
connected peers only, so the driver was connected to all three nodes
(seed endpoint + two LAN endpoints).

---

## 1. Activation performed

| Action | Payload | Broadcast to |
|---|---|---|
| Validator registration | `test_validator` (Active, signed envelope) | node1, node2, node3 |
| Committee selection | `{test_validator}` at state 1 (signed envelope) | node1, node2, node3 |
| Snapshot offer + data | state 1, `testnet-state-1` hash chain | node1, node2, node3 |

Driver-side persistence: validator, committee, and snapshot all written
to the driver node's SQLite (`persist_validator`, `persist_committee`,
`persist_snapshot`).

## 2. Validator propagation — VERIFIED

All three nodes logged receipt within ~1 s of broadcast:

```text
Registered validator: test_validator (signed by xnt-<admin>)   # node1 20:06:35
Registered validator: test_validator (signed by xnt-<admin>)   # node2 20:06:35
Registered validator: test_validator (signed by xnt-<admin>)   # node3 20:06:36
```

Signature, freshness/replay, and connection-identity binding all
enforced on the receiving side (the same `xnt-<admin>` signer appears on
every node — the envelope must match the connection's verified
handshake identity).

**Caveat:** propagation is direct-connection only. `broadcast_message`
iterates the sender's connection table; there is no gossip relay for
`ValidatorRegistration`. A node not directly connected to the
broadcaster learns nothing — in this topology, broadcasts had to be
sent to each node individually.

## 3. Validator persistence — PARTIALLY VERIFIED

| Side | Result |
|---|---|
| Broadcasting node | VERIFIED — SQLite write confirmed; after restart: `STATE validators: 1 active -> ["test_validator"]` |
| Receiving nodes | NOT PERSISTED — receipt updates the in-memory validator manager only; `node2`/`node3` restart loads 0 validators |

The restart-load path is additionally **hardcoded to address
`test_validator`** (`load_persisted_state` queries that literal
address), so validator recovery is only demonstrable for this address.
Any other registered address would not reload even where persisted.

## 4. Validator recovery after restart — PARTIALLY VERIFIED

- **Storage-backed recovery**: verified on the broadcasting node —
  restart → `Loaded validator from storage` → 1 active validator
  (works only because the address is `test_validator`, see §3).
- **Network-level recovery**: verified — after `node2` restart wiped
  its in-memory validator set, re-broadcast restored it within ~1 s
  (`Registered validator` at 20:15:52). No node re-serves known
  validators automatically; recovery depends on someone re-broadcasting.

## 5. Committee propagation & convergence — VERIFIED

All three nodes converged on identical committee state:

```text
Updated committee with 1 members for state 1 (signed by xnt-<admin>)  # node1 20:06:35
Updated committee with 1 members for state 1 (signed by xnt-<admin>)  # node2 20:06:35
Updated committee with 1 members for state 1 (signed by xnt-<admin>)  # node3 20:06:37
```

Every receiver independently verified the signed envelope (signature +
replay + handshake-identity binding) before applying — convergence is
by identical signed input, not by consensus voting.

## 6. Committee persistence & recovery — PARTIALLY VERIFIED

- Driver-side: persisted + reload verified (`Loaded committee from
  storage: 1 members`, real `committee_repo.get()` path — not
  hardcoded).
- Receivers: in-memory only — lost on restart, restored by
  re-broadcast (`Updated committee` re-logged at 21:53:15 on node3).

## 7. Restart scenarios

| Scenario | Observation |
|---|---|
| Validator node restart (node2) | Clean reconnect to node1 in ~1 s; validator/committee/snapshot state EMPTY after restart (nothing persisted); full recovery via re-broadcast ~1 s |
| Non-validator restart (node3) | Identical: graceful stop at 21:40, restarted 21:42 — `Loaded 0 peers`, no validator/committee/snapshot; full state restored via re-broadcast at 21:53:15 |
| Broadcasting node restart | All persisted state reloaded from SQLite (validator via hardcoded-address path, committee + snapshot via real repo paths) |

## 8. Classification

| Area | Classification | Notes |
|---|---|---|
| Validator propagation | VERIFIED | All 3 nodes; signed + auth-bound |
| Validator persistence | PARTIALLY VERIFIED | Origin node only; hardcoded `test_validator` load path |
| Validator recovery | PARTIALLY VERIFIED | Storage reload works for `test_validator` only; network recovery = re-broadcast |
| Committee propagation | VERIFIED | All 3 nodes, identical state 1 |
| Committee convergence | VERIFIED | Signed-input convergence, no voting exercised |
| Committee persistence/recovery | PARTIALLY VERIFIED | Real repo path on origin; receivers memory-only |
| Committee voting | NOT OBSERVED | No `CommitteeVote` messages exist in the activation flow |

## 9. Gaps recorded

| ID | Finding |
|---|---|
| V-1 | Receivers never persist validator/committee — in-memory only; every restart is a full wipe until re-broadcast |
| V-2 | Validator storage reload is hardcoded to address `test_validator` — general persistence recovery is effectively absent |
| V-3 | No gossip relay for registrations/selections — propagation limited to direct connections |
| V-4 | `persist_validator`/`persist_snapshot` use plain `INSERT` — re-registration/re-persist fails on `UNIQUE constraint`; no upsert path |
| V-5 | Nothing in normal node operation self-registers as a validator — activation required the external driver |
