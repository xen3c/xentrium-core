# SECURITY REMEDIATION REPORT — Phase 3 Step 8

Date: 2026-11-17
Source: `PUBLIC_TESTNET_SECURITY_AUDIT.md`
Status: COMPLETE — all Critical and High findings resolved

## Summary

All 3 Critical and 4 High findings from the public testnet security audit have been remediated and verified by 11 new integration tests plus the full suite (200/200 passing).

## Remediations

### C1 — Node identity bound to public key ✅

`NodeIdentity::node_id_for_public_key()` is now public; `verify_handshake()`, `verify_announcement()`, and `verify_envelope()` all reject any payload where `node_id != "xnt-" + sha256(pubkey)[:8]`. Impersonation by claiming a foreign `node_id` is impossible — the ID is a pure function of the key.

Files: `xentrium-network/src/identity.rs`

### C2 — Maximum message size ✅

`MAX_MESSAGE_SIZE = 16 MiB` enforced in `TcpConnection::receive()` **before** allocation — oversized length prefixes are rejected with `ProtocolError` without allocating. Same cap added to `Message::deserialize()`.

Files: `xentrium-network/src/transport.rs`, `xentrium-network/src/protocol.rs`

### C3 — Signed ValidatorRegistration & CommitteeSelection ✅

New `SignedEnvelope` type: domain-separated (`xnt-validator-registration` / `xnt-committee-selection`) signatures over `domain || node_id || timestamp || nonce || pubkey || payload`. Broadcast paths sign; receive paths require:
1. Valid envelope signature + node_id/pubkey binding + 5-min freshness
2. Replay-guard freshness on `(node_id, nonce)`
3. **Connection binding**: envelope `node_id`+`public_key` must equal the connection's handshake-authenticated identity (`TcpConnection.auth_node_id`/`auth_pubkey`, populated on verified handshake). Unsigned legacy payloads are rejected outright.

Files: `xentrium-network/src/identity.rs`, `protocol.rs`, `xentrium-node/src/lib.rs`, `transport.rs`

### H1 — Peer allowlist / identity pinning ✅

`NodeConfig.allowed_node_ids: Vec<String>` + TOML `[network] allowed_node_ids` (serde-default, backward compatible). Enforced on handshake verification (inbound and outbound) and on gossiped announcements. Empty list = open testnet; populated list = invite-only/pinned testnet. Since node_ids are now pubkey-bound (C1), the allowlist pins cryptographic identities.

Files: `xentrium-node/src/lib.rs`, `config.rs`, `main.rs`

### H2 — Signed announcements in PeerList gossip ✅

`SignedAnnouncement` extended with `public_key` + `nonce`; shares the handshake canonical signed-bytes format, so `announcement_from_handshake()` converts verified handshakes into gossipable announcements. `broadcast_peer_list()` now gossips up to 64 verified announcements (own + learned); receivers verify signature, node_id binding, freshness, replay, and allowlist before caching/relaying and converting to `PeerID`s.

Files: `xentrium-network/src/identity.rs`, `protocol.rs`, `xentrium-node/src/lib.rs`

### H3 + H4 — Nonce, replay protection, reduced timestamp tolerance ✅

- `nonce: u64` added to `HandshakePayload`, `SignedAnnouncement`, `SignedEnvelope` (SHA-256-derived unique nonces)
- `ReplayGuard`: bounded (10k entries) `(node_id, nonce)` seen-set with TTL eviction; applied to handshakes, announcements, and envelopes
- Timestamp tolerance reduced from **±24h → ±5min** (`MAX_TIMESTAMP_SKEW_SECS = 300`)

Files: `xentrium-network/src/identity.rs`, `xentrium-node/src/lib.rs`

## Tests Added — `xentrium-node/tests/security_remediation.rs` (11/11 passing)

| Test | Covers |
|------|--------|
| `c1_node_id_must_match_public_key` | C1 — forged node_id rejected in handshake/announcement/envelope |
| `c2_oversized_frame_rejected` | C2 — >16 MiB frame rejected pre-allocation |
| `c3_signed_validator_registration_accepted` | C3 — legitimate signed registration processed end-to-end |
| `c3_unsigned_validator_registration_rejected` | C3 — legacy unsigned payload ignored |
| `c3_wrong_identity_envelope_rejected` | C3 — envelope signer ≠ connection peer rejected |
| `h1_allowlist_rejects_unknown_peer` | H1 — non-listed peer disconnected |
| `h1_allowlist_accepts_listed_peer` | H1 — listed peer stays connected |
| `h2_announcement_sign_verify_tamper_replay` | H2 — sign/verify/tamper/replay unit coverage |
| `h2_peerlist_gossip_signed_announcements` | H2 — announcements propagate via PeerList |
| `h2_forged_announcement_rejected` | H2 — forged announcement not cached |
| `h3_h4_freshness_and_replay` | H3/H4 — stale timestamp + duplicate nonce rejected |

Plus 4 new unit tests in `identity.rs` (node_id binding, timestamp window, replay guard, envelope roundtrip).

## Full Test Results

```
xentrium-consensus: 24/24
xentrium-crypto:    8/8
xentrium-network:  53/53 unit + 40 integration = 93
xentrium-node:     75/75 (incl. 11 security_remediation)
xentrium-shared / xentrium-storage: 0

Total: 200/200 tests passing — zero regressions.
```

## Remaining Limitations (Medium/Low from audit — not testnet-blocking)

- **M1** Rate limiting still absent (message/connection quotas)
- **M2** Identity private key stored unencrypted on disk
- **M3** No certificate rotation/revocation mechanism
- **M4** No hostname verification (IP-based, `localhost` SAN)
- **M5** `machine_id` still self-declared (endpoint now comes from verified announcements)
- **M6** Gossip/discovery tables bounded by TTL+capacity but not hard-quota per peer
- **L1–L3** Log topology exposure, TLS 1.2 permitted, verbose errors

## Readiness Assessment

| Domain | Before | After |
|--------|--------|-------|
| Node impersonation | Exploitable (C1+H1) | **Blocked** — pubkey-bound IDs + optional allowlist |
| Memory DoS | Exploitable (C2) | **Blocked** — 16 MiB frame cap |
| Validator/committee injection | Exploitable (C3) | **Blocked** — signed envelopes + connection binding |
| Eclipse via gossip | Exploitable (H2) | **Blocked** — signed, verified, replay-protected announcements |
| Replay | Open (H3/H4) | **Blocked** — nonces + replay guard + 5-min window |

**Trusted testnet upgraded toward public testnet readiness.** Remaining Medium items (rate limiting, key-at-rest protection, cert rotation) are recommended hardening but no longer permit trivial remote compromise by unauthenticated peers.
