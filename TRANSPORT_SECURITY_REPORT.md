# TRANSPORT SECURITY REPORT — Phase 3 Step 6

Date: 2026-11-17
Status: COMPLETE

## Objective

Add encrypted, authenticated transport to Xentrium node-to-node communication while preserving existing transport APIs, the message/framing protocol, and node runtime compatibility. No consensus changes, no protocol redesign, no QUIC work.

## Transport Evaluation

| Option | Decision | Rationale |
|--------|----------|-----------|
| **rustls 0.23 + tokio-rustls 0.26** | **ADOPTED** | Pure-Rust TLS 1.2/1.3, async-native, integrates cleanly with existing `tokio::net::TcpStream`. Custom `ServerCertVerifier`/`ClientCertVerifier` support enables identity-bound self-signed certs without a CA. |
| quinn (QUIC) | Deferred | QUIC migration was explicitly out of scope for this step (per TAD roadmap, QUIC remains a later transport upgrade). Prior feasibility assessment documented ecosystem friction. TLS-over-TCP delivers the same encryption/authentication guarantees on the current transport. |
| Alternative TLS (native-tls/openssl) | Rejected | OpenSSL bindings add platform-specific build complexity on Windows; rustls is already in the dependency tree via prior evaluation and has no C dependencies. |

## Implementation

### New module: `xentrium-network/src/tls.rs`

- **`TlsCredentials`** — self-signed certificate (rcgen) with `CN = node_id`. Persisted as a single `tls_credentials.bin` file (cert+key length-prefixed blob) written atomically via temp-file + rename, preventing cert/key mismatch under concurrent access. `load_or_generate()` validates the persisted pair by building a `ServerConfig` before reuse; corrupt files are regenerated.
- **`XentriumCertVerifier`** (`ServerCertVerifier`) — client-side verification of the server certificate: parses X.509 via `x509-parser`, rejects malformed/expired certs, extracts `CN` as the peer's node identity. Trust model: testnet TOFU — any well-formed, unexpired cert is accepted at the TLS layer; cryptographic identity binding is enforced by cross-checking cert CN against the signed Ed25519 Xentrium handshake.
- **`XentriumClientCertVerifier`** (`ClientCertVerifier`) — server-side mutual-TLS verification: requires clients to present a well-formed, unexpired certificate with a CN. This makes authentication **mutual**: both peers prove certificate identity.
- **`TlsTransportConfig`** — bundles `TlsAcceptor` + `TlsConnector`; server name `localhost`.
- Certificate validity window: 2025-01-01 → 2035-01-01 (10-year testnet certs).

### Modified: `xentrium-network/src/transport.rs`

- New `IoStream` trait (`AsyncRead + AsyncWrite + Unpin + Send + Sync`) abstracting plain `TcpStream` and `tokio_rustls::TlsStream<TcpStream>`.
- `TcpConnection.stream` is now `Box<dyn IoStream>`; `send`/`receive`/`close` and the 4-byte length-prefixed framing are unchanged — the same message protocol runs inside the TLS record layer.
- `TcpConnection.tls_peer_node_id: Option<String>` exposes the peer's certificate CN after handshake.
- `TcpTransport::enable_tls(TlsTransportConfig)` — opt-in; transports without TLS behave exactly as before (API preserved).
- `connect()` performs the TLS client handshake when enabled and records the server cert's CN; `accept(&self)` performs the TLS server handshake (signature changed `&mut self` → `&self`, source-compatible for callers) and records the client cert's CN.

### Modified: `xentrium-node/src/lib.rs`

- `start_transport()` loads/generates `TlsCredentials` from `<data_dir>/tls/` bound to `identity.node_id` and enables TLS on the transport — **all node traffic is now mutually authenticated TLS**.
- Handshake handler cross-checks `connection.tls_peer_node_id` (cert CN) against the Ed25519-signed `HandshakePayload.node_id`; mismatch → connection dropped. This binds the TLS certificate identity to the persistent node keypair.
- Accept loop switched to `transport.read()` lock: required because `connect_to_bootstrap_peers` holds a read lock across the TLS handshake await; `TcpListener::accept` only needs `&self`, eliminating a handshake deadlock.

## Verification

### Integration tests — `xentrium-node/tests/transport_security.rs` (6/6 passing)

| Test | Result |
|------|--------|
| `tls_handshake_success` | PASS — mutual TLS handshake completes both directions |
| `tls_handshake_failure_plaintext_client` | PASS — plaintext bytes sent to TLS acceptor are rejected |
| `tls_handshake_failure_tls_client_plaintext_server` | PASS — TLS client cannot handshake with plaintext server (error/timeout) |
| `certificate_validation` | PASS — valid cert yields CN; malformed DER and expired certs rejected |
| `invalid_certificate_rejection` | PASS — TLS client rejects garbage/non-TLS endpoint |
| `encrypted_ping_pong` | PASS — Ping/Pong round-trip over TLS; both ends observe peer cert CN (`node-alpha`/`node-beta`) |

### Full suite

```
xentrium-consensus: 24/24
xentrium-crypto:    8/8
xentrium-network:  89/89 (49 unit + 40 integration)
xentrium-node:     64/64 (6 unit + 25 multi_node + 6 durability + 6 node_operation
                        + 7 heartbeat + 6 transport_security + audits)
xentrium-shared / xentrium-storage: 0 tests

Total: 185/185 tests passing — including all 25 multi-node tests,
which now run entirely over mutual TLS.
```

## Security Properties Achieved

- **Encryption**: all node-to-node bytes are inside TLS 1.3 (TLS 1.2 also permitted by safe defaults) records; eavesdroppers see only ciphertext.
- **Authentication (mutual)**: server verifies client certificate; client verifies server certificate. Both certs carry `CN = node_id`.
- **Identity binding**: cert CN is cross-checked against the Ed25519-signed Xentrium handshake payload; a peer cannot present a valid cert for a different node identity.
- **Rejection**: malformed, expired, or non-TLS endpoints fail the handshake before any application message is processed.
- **Persistence**: TLS credentials survive restarts alongside `node.identity`.

## Known Limitations / Caveats

- **TOFU trust model**: the verifier accepts any well-formed, unexpired certificate (no CA or pinned peer list yet). A determined MITM cannot forge a cert whose CN matches the victim's handshake node_id without also breaking the Ed25519 signature — but there is no out-of-band cert pinning. Certificate pinning / allowlists are recommended before public internet exposure.
- **No hostname verification**: server name is fixed to `localhost`; IP-based dialing is used with SAN `localhost`. Acceptable for testnet; revisit for WAN deployment.
- **Self-signed 10-year certs**: no rotation mechanism yet.
- **Parallel same-directory writers**: two nodes sharing one `data_directory` may race credential generation; the atomic single-file write guarantees readers never see a mismatched pair, and each node uses its own in-memory credentials consistently.
- Plaintext `TcpTransport` remains available via `TcpTransport::new()` (no `enable_tls`) for tests/tools; the node runtime always enables TLS.

## Conclusion

**PHASE-3 STEP-6 is COMPLETE.** All node communication is now encrypted (TLS over the existing TCP transport) and mutually authenticated (certificate CN bound to the persistent Ed25519 node identity via the signed handshake). Existing transport APIs, message framing, schedulers, heartbeat, discovery, and consensus logic are unchanged and fully compatible — verified by 185/185 passing tests.
