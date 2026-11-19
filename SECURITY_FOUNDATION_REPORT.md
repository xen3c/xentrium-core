# Security Foundation Report

**Date:** 2026-11-16
**Phase:** PHASE-3 STEP-5 - Network Security Foundation
**Protocol Version:** TAD v2.2.7 (Frozen)

---

## Summary

Xentrium nodes now possess persistent cryptographic identity (Ed25519), exchange signed handshakes on every connection, verify peer identity before participation, support signed peer announcements, and reject malformed/invalid peers.

---

## Files Created

| File | Purpose |
|------|---------|
| `xentrium-network/src/identity.rs` | `NodeIdentity`, `HandshakePayload`, `SignedAnnouncement` — key generation, persistence, handshake create/verify, announcement sign/verify |
| `xentrium-node/tests/security_foundation.rs` | 5 integration tests |
| `SECURITY_FOUNDATION_REPORT.md` | This report |

## Files Modified

| File | Changes |
|------|---------|
| `xentrium-network/src/lib.rs` | Exported `identity` module |
| `xentrium-network/src/protocol.rs` | `serialize_handshake()` / `deserialize_handshake()` (bincode) |
| `xentrium-network/Cargo.toml` | Added `hex`, `chrono`, `ed25519-dalek`, `sha2` deps |
| `xentrium-crypto/src/keys.rs` | `KeyPair` now `Clone` |
| `xentrium-node/src/lib.rs` | `identity: NodeIdentity` field; loaded via `load_or_create(data_dir/node.identity)` in both constructors; handshake sent on outbound connect and inbound accept; `Handshake` messages verified in receive loop — invalid → connection removed; `identity()` getter |

---

## Features Implemented

### 1. Peer Identity System
- `NodeIdentity` wraps an Ed25519 `KeyPair`
- `node_id` derived deterministically: `xnt-<first 8 bytes of SHA-256(pubkey) hex>`
- Public key: 32 bytes; signatures: 64 bytes (ed25519-dalek)

### 2. Node Keypair Generation
- `NodeIdentity::generate()` — OS RNG via `SigningKey::generate(&mut OsRng)`

### 3. Identity Persistence
- `load_or_create(path)` — loads hex-encoded 64-byte keypair from `<data_dir>/node.identity`, or generates + saves if absent
- Survives restarts; same node_id/public_key across loads (verified)

### 4. Node Handshake
- `create_handshake(endpoint)` produces `HandshakePayload { node_id, public_key, endpoint, timestamp, signature }`
- Signature covers canonical bytes: `node_id | 0x00 | endpoint | 0x00 | timestamp_be | pubkey`
- Serialized via bincode into `MessageType::Handshake` payload
- Sent automatically: outbound bootstrap connect AND inbound accept

### 5. Peer Identity Verification
- `verify_handshake()` validates structure (non-empty node_id ≤128B, 32B pubkey, non-empty endpoint ≤256B, 64B signature, timestamp within ±24h) then verifies Ed25519 signature
- Receive loop: valid → logged as verified peer; invalid → connection removed (rejected)

### 6. Signed Peer Announcements
- `sign_announcement(endpoint)` / `verify_announcement(ann, pubkey)` — same canonical signing scheme, usable for gossiping authenticated peer endpoints

### 7. Malformed Peer Rejection
- Empty/oversized node_id, wrong pubkey/signature lengths, stale timestamps, corrupted signatures, foreign-key signatures → all rejected (`InvalidSignature`)
- Rejected peers have their TCP connection dropped by the receive loop

---

## Integration Tests (5/5 passing)

| Test | Covers |
|------|--------|
| `test_identity_generation_and_persistence` | generation + disk persistence across loads |
| `test_handshake_success` | create → serialize → deserialize → verify roundtrip |
| `test_handshake_rejection_malformed` | bad pubkey/sig/node_id/timestamp rejected |
| `test_invalid_signature_rejection` | corrupted sig + foreign-key tampering rejected |
| `test_two_nodes_handshake_and_identity` | live 2-node: distinct identities, persisted files, connection survives handshake |

Plus 6 unit tests inside `identity.rs`.

---

## Test Results

```
security_foundation.rs: 5/5 passed (~3.3s)
identity.rs unit tests: 6/6 passed
```

---

## Constraints Compliance

- ✅ No QUIC, no TLS, no protocol redesign
- ✅ Uses existing MessageType::Handshake + TCP transport
- ✅ Ed25519 via existing xentrium-crypto

---

## Known Limitations

1. **Handshake is one-shot at connect time** — no periodic re-verification or key rotation.
2. **PeerList gossip still carries unauthenticated PeerIDs** — `SignedAnnouncement` exists but isn't yet wired into PeerList exchange.
3. **Verified peer state isn't persisted** — identity verification result lives in the connection, not stored in `peers` table.
4. **No MITM protection without TLS** — signatures prove key ownership but the channel itself is plaintext; an active MITM could still relay. TLS remains the next step.
5. **Node identity ≠ validator identity** — this is a network-layer identity; validator keys remain separate per TAD.

---

## Success Criteria Assessment

| Criterion | Status |
|-----------|--------|
| Nodes possess persistent cryptographic identity | ✅ Ed25519 keypair persisted to disk |
| Verify remote peers before allowing participation | ✅ Handshake signature verified; invalid peers disconnected |

**PHASE-3 STEP-5: COMPLETE**
