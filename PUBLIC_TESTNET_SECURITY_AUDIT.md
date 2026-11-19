# PUBLIC TESTNET SECURITY AUDIT — Phase 3 Step 7

Date: 2026-11-17
Scope: identity, TLS, peer authentication, persistence, discovery, validator propagation
Method: code review of `xentrium-network` (identity, tls, transport, discovery, gossip, heartbeat, protocol) and `xentrium-node` (message handlers, schedulers). Audit only — no implementation.

## Executive Summary

Xentrium now has encrypted, mutually-authenticated transport (TLS 1.3, identity-bound certificates, signed handshakes). However, **the authentication chain has a critical binding flaw**: `node_id` is not verified to be derived from the peer's public key, and the TLS trust model is TOFU (any well-formed self-signed cert accepted). Combined with unsigned validator/committee messages and an uncapped message-length prefix, the node is **not safe for public internet exposure**. It remains suitable for trusted/private testnets.

## Findings

### CRITICAL

**C1 — Node identity is not bound to the public key (impersonation)**
`NodeIdentity::verify_handshake()` verifies the Ed25519 signature over `node_id || endpoint || timestamp || pubkey` but never checks that `node_id == "xnt-" + sha256(pubkey)[:8]`. An attacker generates any keypair, signs a handshake claiming *any* `node_id` (e.g. a victim validator's), and presents a self-signed TLS cert with `CN` set to that same `node_id`. Both the cert-CN check and the signature check pass → **full node impersonation**, including impersonating validators.
File: `xentrium-network/src/identity.rs:127-154`, binding check `xentrium-node/src/lib.rs:~782`.

**C2 — Unbounded message length prefix (memory DoS)**
`TcpConnection::receive()` reads a u32 length and immediately allocates `vec![0u8; len]` — up to ~4 GiB per message, per connection, with no cap and no rate limit. A single malicious peer can exhaust memory.
File: `xentrium-network/src/transport.rs:167-181`.

**C3 — Validator registration and committee selection are unauthenticated (Sybil → consensus capture)**
`ValidatorRegistration` and `CommitteeSelection` payloads are deserialized and applied with **no signature verification and no sender authorization**. Any connected peer can register arbitrary validators or overwrite the committee. In bootstrap phase this directly enables committee/validator-state manipulation; at 21+ validators it threatens consensus inputs.
File: `xentrium-node/src/lib.rs:839-859`.

### HIGH

**H1 — TOFU certificate trust: no pinning, allowlist, or CA**
`XentriumCertVerifier` accepts *any* well-formed, unexpired certificate. There is no peer allowlist, no cert pinning, no revocation. Combined with C1 this makes impersonation trivial; even without C1, any node can join the mesh.
File: `xentrium-network/src/tls.rs:99-137`.

**H2 — PeerList gossip is unsigned (eclipse/Sybil amplification)**
`receive_peer_list()` accepts `Vec<PeerID>` with no signature or provenance check. `SignedAnnouncement` exists but is **not used in gossip**. An attacker can flood peers with fabricated endpoints, steering victims toward eclipse or wasting connection attempts.
Files: `xentrium-network/src/discovery.rs:145`, `xentrium-network/src/gossip.rs`.

**H3 — No replay/nonce protection on handshake or announcements**
Timestamp window is ±24h (`identity.rs:143`, `:179`) with no nonce or seen-set. Handshakes/announcements captured within the window can be replayed. TLS protects the live channel, but signed announcements reused in gossip (once wired) would be replayable.

**H4 — Handshake timestamp skew too permissive**
±86400s tolerance is far beyond reasonable clock skew; enables pre-computed/replayed handshakes and weakens freshness guarantees. Should be minutes, not a day.

### MEDIUM

**M1 — No inbound connection/message rate limiting**
No cap on concurrent TLS handshakes, messages per second per peer, or total connections beyond config `max_peers` (enforcement unverified on accept path). Ping floods force Pong responses; handshake floods consume CPU.

**M2 — Node identity private key stored unencrypted**
`node.identity` is hex-encoded raw Ed25519 keypair on disk with default filesystem permissions. Standard for server keys, but should be documented; consider OS keychain/DPAPI or file-permission hardening for production.

**M3 — Self-signed 10-year certificates, no rotation/revocation**
Certs valid 2025→2035; compromise requires manual re-keying. No mechanism to rotate `tls_credentials.bin` or `node.identity`.

**M4 — No hostname/SAN verification**
Server name is hardcoded `localhost`; connections are IP-based. Fine for testnet, incorrect for WAN deployment where endpoint identity should be verified.

**M5 — `endpoint`/`machine_id` fields are self-declared**
`PeerID.machine_id` and handshake `endpoint` are attacker-controlled strings; actual TCP source address is used for accepted sockets (good), but gossiped endpoints are trusted blindly (see H2).

**M6 — Gossip `seen_messages` / discovery tables grow unboundedly**
Dedup/expiry cleanup exists (`cleanup_expired`) but is interval-based; a flood between cleanups grows memory.

### LOW

**L1 — Logging may leak topology**
Peer endpoints and node IDs are logged at info level; acceptable for testnet, review for production.

**L2 — TLS 1.2 still permitted**
`with_safe_default_protocol_versions` allows TLS 1.2; consider TLS 1.3-only for production.

**L3 — Error messages distinguish failure causes**
TLS/handshake errors are logged with detail; minor information disclosure, low risk.

## Attack Vector Summary

| Vector | Status | Primary gap |
|--------|--------|-------------|
| Eavesdropping | MITIGATED | TLS 1.3 encrypts all traffic |
| Node impersonation | **OPEN** | C1 + H1 |
| Validator/committee injection | **OPEN** | C3 |
| Memory exhaustion | **OPEN** | C2 |
| Eclipse via gossip | **OPEN** | H2 |
| Replay | PARTIAL | H3/H4 (TLS mitigates channel replay) |
| Sybil | **OPEN** | C1, C3, H2, no economic barrier |
| MITM | PARTIAL | TLS encryption holds; identity binding broken by C1/H1 |

## Minimum Fixes Before Public Testnet

1. **C1**: verify `node_id` derives from `public_key` in `verify_handshake()` (and announcements).
2. **C2**: enforce a maximum message size (e.g. 16 MiB) before allocation; reject oversized frames.
3. **C3**: require validator/committee messages to carry a signature verifiable against the sender's handshake identity; reject unsigned registrations.
4. **H1**: add a bootstrap-peer/node allowlist or cert pinning for the initial testnet.
5. **H2**: sign PeerList announcements with `SignedAnnouncement` and verify before ingesting.
6. **H3/H4**: reduce timestamp window to ~5 min and add nonce/seen-set replay protection.

## Minimum Fixes Before Production

- Replace TOFU with a CA or web-of-trust/allowlist governance for certificate issuance.
- Certificate rotation, revocation, and expiry alerting.
- Per-peer and global rate limiting; connection caps enforced at accept.
- Economic Sybil resistance (stake/bond for validator registration) per TAD.
- MachineID verification and endpoint cross-checks.
- TLS 1.3-only policy; hostname verification for WAN.
- Memory-bounded gossip/discovery tables; message quotas.
- Security audit of snapshot install path (payload size, hash verification before state replacement).

## Conclusion

**NOT READY for public internet deployment.** Encryption is in place, but identity binding (C1), unauthenticated validator/committee propagation (C3), unbounded allocation (C2), and unsigned gossip (H2) are exploitable by any internet-connected peer. Estimated effort for the testnet-blocking fixes: ~1–2 weeks. Production readiness additionally requires the CA/trust, rate-limiting, and economic-barrier work listed above.
