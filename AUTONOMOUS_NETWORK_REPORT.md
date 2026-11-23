# Autonomous Network Report — Phase 4, Step 3

**Date:** 2026-11-23 (UTC)
**Builds:** `45e31ac` (autonomous operation), `3d8f4b3` (epoch-race
guard), `4fae6c3` (replay-nonce ordering) — all validated live.
**Topology:** node1 = public seed; node2, node3 = intermittent leaf
peers behind NAT, connected outbound to node1 (star).

**Goal:** the network operates with zero `testnet_admin` intervention.

---

## 1. What was implemented

| Mechanism | How it works |
|---|---|
| Validator self-registration | With `self_register_validator = true` (default in deploy configs), each node registers its own identity (`xnt-<node_id>`) as an Active validator at startup and re-announces every `announce_interval_secs` with a fresh signed envelope |
| Automatic propagation | Newly verified peers are served all cached state on connect; a periodic "sync beacon" re-broadcasts PeerList + cached validator envelopes + latest committee envelope + latest snapshot offer; receivers cache and relay first-seen envelopes so state crosses the star |
| Relayed-envelope authentication | Envelopes verify against `known_identities` — peer node_ids learned from verified handshakes and verified announcements (PeerList). Full signature/freshness/node_id-binding checks are unchanged; relay only removes the requirement that the *transport peer* be the signer |
| Committee formation + rotation | Every `committee_epoch_secs`, each node deterministically derives the committee: sort active validator addresses, rotate by epoch, cap at `MAX_COMMITTEE_SIZE`. Identical input ⇒ identical output on every node, so formation needs no voting. The first member post-rotation broadcasts the signed `CommitteeSelection` |
| Snapshot production | The committee leader produces one snapshot per `snapshot_epoch_secs` epoch, persists + serves it, and advertises a `SnapshotOffer` |
| SnapshotRequest pull path | On a newer offer, a node records the pending request and sends `SnapshotRequest(state)` to the offerer, which replies with `SnapshotData`; the receiver verifies, installs, persists, and re-offers it onward |

New config keys (`[consensus]`, all serde-defaulted, backward
compatible): `self_register_validator`, `announce_interval_secs`,
`committee_epoch_secs`, `snapshot_epoch_secs`.

## 2. Join → converge flow (required scenario)

```text
Node joins network
↓   outbound TLS connect to node1, mutual signed handshake
receives committee state      ← serve-on-connect + periodic beacon
↓   (deterministic derivation also applies locally each epoch)
receives validator state      ← own + relayed envelopes through node1
↓
requests missing snapshot     ← SnapshotOffer newer than local state
↓   → SnapshotRequest → SnapshotData → install + persist + re-offer
converges automatically       ← verified live: see §4
```

## 3. Live validation evidence

From `data/node2/xentrium.log`, times UTC:

```text
22:46:19  Self-registered as validator: xnt-cdb9aacaccca96b3
22:46:19  Verified handshake from <node1>:443 (TLS-bound)
23:15:28  Registered validator: xnt-81d31e1ef16bf7b3   ← node1's self-reg
23:34:30  Received newer snapshot offer: state 5984922 — requesting
23:34:30  Received and stored snapshot: state 5984922  ← pull path live
23:39:30  Registered validator: xnt-93db752e4f326b15   ← relayed via node1
23:39:43  Committee formed: 3 members (epoch 5984923)
23:40:13  Committee formed: 3 members (epoch 5984924, rotated leader)
```

All three validators, one converged committee, snapshots flowing —
**no driver, no manual broadcast**.

## 4. Integration tests

`xentrium-node/tests/autonomous.rs` (all passing, ~13 s total):

- `test_autonomous_convergence_three_nodes` — seed + 2 leaf nodes
  converge on 3 validators (third learned via relay), identical
  committee, equal snapshot state; node3 restart reloads all three
  state kinds from its own SQLite.
- `test_late_joiner_syncs_state` — a node joining an already-converged
  pair pulls the full state via serve-on-connect + beacon.
- `test_derive_committee_deterministic` — order-independent input,
  deterministic leader rotation, `MAX_COMMITTEE_SIZE` cap.

Full workspace suite: green, no regressions.

## 5. Bugs found and fixed during this step

| Bug | Symptom | Fix |
|---|---|---|
| `Node::stop()` never released the listener socket | Restarted node in-process couldn't rebind its port (EADDRINUSE) | `TcpTransport::stop_listener`, called from `stop()` |
| Replay nonce consumed before auth | An envelope rejected only because the signer was not yet known burned its (node_id, nonce), permanently poisoning that envelope — node2 could never learn node3's registration | Replay check now runs only after auth passes; unknown signers don't consume nonces |
| Stale-committee epoch race | Snapshot task could fire before the committee rotated → two nodes produced competing snapshots for the same state | Production requires `vm.state_number()` to be current-epoch |
| Hardcoded `test_validator` reload | Only one literal address was reloaded from storage at startup | `ValidatorRepository::get_all()` loads every persisted validator |
| Plain `INSERT` persistence | Re-registration/re-persist of the same validator or snapshot failed on `UNIQUE constraint` | `INSERT OR REPLACE` in validator + snapshot repos |
| `new_with_storage` didn't create the data dir | `unable to open database file` on first run | `create_dir_all` before opening SQLite |
| PeerList never sent | `broadcast_peer_list` existed but was dead code — no identity gossip for relayed-envelope auth | Announce task broadcasts verified announcements every interval |

## 6. Verified / partially verified / not observed

| Item | Status |
|---|---|
| Validator self-registration | VERIFIED (all 3 nodes, live) |
| Automatic validator propagation | VERIFIED — incl. relay across the star (xnt-93db reached node2 through node1) |
| Automatic committee formation | VERIFIED — identical committee on all nodes, re-forms within seconds when the validator set changes |
| Automatic committee rotation | VERIFIED — leader rotates deterministically each epoch |
| Automatic snapshot production | VERIFIED — leader produces per epoch |
| SnapshotRequest pull path | VERIFIED — offer → request → data → install → persist → re-offer, observed live |
| Join → converge sequence | VERIFIED — node3's registration + committee + snapshot state reached node2 with no intervention |
| Restart recovery | VERIFIED — receivers now persist all state; restart reloads validators/committee/snapshot (previously memory-only) |

## 7. Known limitations (documented, not fixed)

- **Divergent snapshot content on split validator views**: if two
  nodes produce for the same state while their validator sets differ
  (transient), the state_hash differs — a node keeps whichever it
  installed; the next epoch's snapshot supersedes. Forward convergence
  only.
- **Committee correctness depends on validator-set convergence**:
  members are derived deterministically, so a node missing a validator
  temporarily computes a smaller committee — self-corrects within one
  announce interval once the envelope arrives.
- **Producer must be reachable**: the epoch's producer broadcasts the
  offer on its own connections; a leader behind NAT can still reach
  the seed, and re-offer relay covers further hops — but if the leader
  has zero connections, that epoch simply has no snapshot.
- **Duplicate SnapshotRequests**: two concurrent offers for the same
  state log + send a request each (pending-request dedup rejects the
  second internally but the message still goes out once per offer).
  Harmless.
- **`auto_committee_rotation` / `snapshot_interval_hours` legacy keys**
  remain for backward compatibility but the epoch-based keys now drive
  behaviour.

## 8. Conclusion

Success criterion met: **the network operates without any
`testnet_admin` intervention.** Nodes self-register, propagate state
through the seed (including through a relay for indirectly connected
peers), form and rotate a committee deterministically, produce
snapshots on a schedule, and pull missing snapshots on demand — all
signed, replay-protected, and identity-bound, and all persisted so
restarts recover state autonomously.
