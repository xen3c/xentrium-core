use xentrium_crypto::{KeyPair, verify_signature};
use xentrium_shared::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Maximum acceptable clock skew / freshness window for signed messages (5 minutes)
pub const MAX_TIMESTAMP_SKEW_SECS: i64 = 300;

/// Persistent cryptographic node identity (Ed25519)
#[derive(Clone)]
pub struct NodeIdentity {
    /// Human-readable node ID derived from public key
    pub node_id: String,
    keypair: KeyPair,
}

/// Handshake payload exchanged between peers
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HandshakePayload {
    pub node_id: String,
    pub public_key: Vec<u8>,
    pub endpoint: String,
    pub timestamp: i64,
    pub nonce: u64,
    pub signature: Vec<u8>,
}

/// Signed peer announcement (self-signed identity advertisement for gossip)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SignedAnnouncement {
    pub node_id: String,
    pub public_key: Vec<u8>,
    pub endpoint: String,
    pub timestamp: i64,
    pub nonce: u64,
    pub signature: Vec<u8>,
}

/// Signed envelope wrapping an arbitrary message payload (validator registration,
/// committee selection, etc.). The signature covers domain || identity || payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SignedEnvelope {
    pub node_id: String,
    pub public_key: Vec<u8>,
    pub timestamp: i64,
    pub nonce: u64,
    pub signature: Vec<u8>,
    pub payload: Vec<u8>,
}

/// Replay protection: tracks seen (node_id, nonce) pairs and rejects duplicates.
/// Entries older than the freshness window are evicted; capacity is bounded.
#[derive(Debug)]
pub struct ReplayGuard {
    seen: HashMap<(String, u64), Instant>,
    capacity: usize,
    ttl: Duration,
}

impl ReplayGuard {
    pub fn new(capacity: usize) -> Self {
        Self {
            seen: HashMap::new(),
            capacity,
            ttl: Duration::from_secs((MAX_TIMESTAMP_SKEW_SECS as u64) * 4),
        }
    }

    /// Returns true if the (node_id, nonce) pair is fresh and was recorded;
    /// false if it was already seen (replay).
    pub fn check_and_insert(&mut self, node_id: &str, nonce: u64) -> bool {
        let key = (node_id.to_string(), nonce);
        if self.seen.contains_key(&key) {
            return false;
        }
        // Evict expired entries
        let ttl = self.ttl;
        self.seen.retain(|_, t| t.elapsed() < ttl);
        // Evict oldest if still over capacity
        while self.seen.len() >= self.capacity {
            let oldest = self.seen.iter()
                .min_by_key(|(_, t)| *t)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => { self.seen.remove(&k); }
                None => break,
            }
        }
        self.seen.insert(key, Instant::now());
        true
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }
}

/// Generate a unique nonce (SHA-256 of time + counter)
fn next_nonce() -> u64 {
    use sha2::{Digest, Sha256};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut h = Sha256::new();
    h.update(nanos.to_le_bytes());
    h.update(counter.to_le_bytes());
    let digest = h.finalize();
    u64::from_le_bytes(digest[..8].try_into().unwrap())
}

impl NodeIdentity {
    /// Generate a new random identity
    pub fn generate() -> Self {
        let keypair = KeyPair::generate();
        let node_id = Self::derive_node_id(&keypair);
        Self { node_id, keypair }
    }

    /// Load identity from a 64-byte keypair file, or generate+persist a new one
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let bytes = std::fs::read(path)
                .map_err(|e| xentrium_shared::XentriumError::DatabaseError(
                    format!("Failed to read identity file: {}", e)))?;
            // File may be hex-encoded or raw bytes
            let raw = if bytes.len() == 128 {
                // hex encoded 64 bytes
                hex::decode(String::from_utf8_lossy(&bytes).as_bytes())
                    .map_err(|_| xentrium_shared::XentriumError::DatabaseError(
                        "Invalid hex in identity file".to_string()))?
            } else {
                bytes
            };
            if raw.len() != 64 {
                return Err(xentrium_shared::XentriumError::DatabaseError(
                    format!("Invalid identity key length: {}", raw.len())));
            }
            let keypair = KeyPair::from_bytes(&raw)?;
            let node_id = Self::derive_node_id(&keypair);
            Ok(Self { node_id, keypair })
        } else {
            let identity = Self::generate();
            identity.save(path)?;
            Ok(identity)
        }
    }

    /// Persist identity to disk (hex-encoded keypair)
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| xentrium_shared::XentriumError::DatabaseError(
                    format!("Failed to create identity dir: {}", e)))?;
        }
        let hex_bytes = hex::encode(self.keypair.to_bytes());
        std::fs::write(path, hex_bytes)
            .map_err(|e| xentrium_shared::XentriumError::DatabaseError(
                format!("Failed to write identity file: {}", e)))?;
        Ok(())
    }

    /// Derive node_id from public key bytes (first 16 hex chars of SHA-256 of pubkey)
    pub fn node_id_for_public_key(pubkey: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(pubkey);
        format!("xnt-{}", hex::encode(&hash[..8]))
    }

    /// Derive node_id from keypair
    fn derive_node_id(keypair: &KeyPair) -> String {
        Self::node_id_for_public_key(&keypair.public_key().to_bytes())
    }

    pub fn public_key_bytes(&self) -> Vec<u8> {
        self.public_key().to_bytes().to_vec()
    }

    pub fn public_key(&self) -> ed25519_dalek::VerifyingKey {
        self.keypair.public_key()
    }

    /// Canonical bytes signed in handshakes and announcements
    fn signed_bytes(node_id: &str, endpoint: &str, timestamp: i64, nonce: u64, public_key: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(node_id.as_bytes());
        data.push(0);
        data.extend_from_slice(endpoint.as_bytes());
        data.push(0);
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(&nonce.to_be_bytes());
        data.extend_from_slice(public_key);
        data
    }

    /// Canonical bytes signed in envelopes (domain-separated)
    fn envelope_signed_bytes(domain: &str, node_id: &str, timestamp: i64, nonce: u64, public_key: &[u8], payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(domain.as_bytes());
        data.push(0);
        data.extend_from_slice(node_id.as_bytes());
        data.push(0);
        data.extend_from_slice(&timestamp.to_be_bytes());
        data.extend_from_slice(&nonce.to_be_bytes());
        data.extend_from_slice(public_key);
        data.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        data.extend_from_slice(payload);
        data
    }

    /// Create a signed handshake payload for the given endpoint
    pub fn create_handshake(&self, endpoint: &str) -> HandshakePayload {
        let timestamp = chrono::Utc::now().timestamp();
        let nonce = next_nonce();
        let pubkey = self.public_key_bytes();
        let signed = Self::signed_bytes(&self.node_id, endpoint, timestamp, nonce, &pubkey);
        let signature = self.keypair.sign(&signed);
        HandshakePayload {
            node_id: self.node_id.clone(),
            public_key: pubkey,
            endpoint: endpoint.to_string(),
            timestamp,
            nonce,
            signature,
        }
    }

    /// Verify a handshake payload; returns Err on any malformed/invalid input.
    /// Enforces: structure, node_id derived from public key, freshness (<=5 min skew), signature.
    pub fn verify_handshake(payload: &HandshakePayload) -> Result<()> {
        // Structural validation
        if payload.node_id.is_empty() || payload.node_id.len() > 128 {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        if payload.public_key.len() != 32 {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        if payload.endpoint.is_empty() || payload.endpoint.len() > 256 {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        if payload.signature.len() != 64 {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        // C1: node_id must be deterministically derived from the public key
        if payload.node_id != Self::node_id_for_public_key(&payload.public_key) {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        // H4: reject stale/future timestamps beyond tight skew window
        let now = chrono::Utc::now().timestamp();
        if (payload.timestamp - now).abs() > MAX_TIMESTAMP_SKEW_SECS {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        // Verify signature
        let mut pk_bytes = [0u8; 32];
        pk_bytes.copy_from_slice(&payload.public_key);
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes)
            .map_err(|_| xentrium_shared::XentriumError::InvalidSignature)?;
        let signed = Self::signed_bytes(
            &payload.node_id, &payload.endpoint, payload.timestamp, payload.nonce, &payload.public_key);
        verify_signature(&verifying_key, &signed, &payload.signature)
    }

    /// Sign a peer announcement
    pub fn sign_announcement(&self, endpoint: &str) -> SignedAnnouncement {
        let timestamp = chrono::Utc::now().timestamp();
        let nonce = next_nonce();
        let pubkey = self.public_key_bytes();
        let signed = Self::signed_bytes(&self.node_id, endpoint, timestamp, nonce, &pubkey);
        let signature = self.keypair.sign(&signed);
        SignedAnnouncement {
            node_id: self.node_id.clone(),
            public_key: pubkey,
            endpoint: endpoint.to_string(),
            timestamp,
            nonce,
            signature,
        }
    }

    /// Build a SignedAnnouncement from a verified handshake payload.
    /// Valid because handshake and announcement share the same canonical signed bytes.
    pub fn announcement_from_handshake(payload: &HandshakePayload) -> SignedAnnouncement {
        SignedAnnouncement {
            node_id: payload.node_id.clone(),
            public_key: payload.public_key.clone(),
            endpoint: payload.endpoint.clone(),
            timestamp: payload.timestamp,
            nonce: payload.nonce,
            signature: payload.signature.clone(),
        }
    }

    /// Verify a signed peer announcement (self-contained: embedded pubkey)
    pub fn verify_announcement(announcement: &SignedAnnouncement) -> Result<()> {
        if announcement.node_id.is_empty() || announcement.endpoint.is_empty() {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        if announcement.public_key.len() != 32 || announcement.signature.len() != 64 {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        // C1: node_id must be derived from the embedded public key
        if announcement.node_id != Self::node_id_for_public_key(&announcement.public_key) {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        // H4: freshness window
        let now = chrono::Utc::now().timestamp();
        if (announcement.timestamp - now).abs() > MAX_TIMESTAMP_SKEW_SECS {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        let mut pk_bytes = [0u8; 32];
        pk_bytes.copy_from_slice(&announcement.public_key);
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes)
            .map_err(|_| xentrium_shared::XentriumError::InvalidSignature)?;
        let signed = Self::signed_bytes(
            &announcement.node_id, &announcement.endpoint,
            announcement.timestamp, announcement.nonce, &announcement.public_key);
        verify_signature(&verifying_key, &signed, &announcement.signature)
    }

    /// Sign an arbitrary payload inside a domain-separated envelope
    pub fn sign_envelope(&self, domain: &str, payload: &[u8]) -> SignedEnvelope {
        let timestamp = chrono::Utc::now().timestamp();
        let nonce = next_nonce();
        let pubkey = self.public_key_bytes();
        let signed = Self::envelope_signed_bytes(
            domain, &self.node_id, timestamp, nonce, &pubkey, payload);
        let signature = self.keypair.sign(&signed);
        SignedEnvelope {
            node_id: self.node_id.clone(),
            public_key: pubkey,
            timestamp,
            nonce,
            signature,
            payload: payload.to_vec(),
        }
    }

    /// Verify a signed envelope; returns the inner payload on success.
    /// Enforces: structure, node_id binding, freshness, signature, domain match.
    pub fn verify_envelope(envelope: &SignedEnvelope, domain: &str) -> Result<Vec<u8>> {
        if envelope.node_id.is_empty() || envelope.node_id.len() > 128 {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        if envelope.public_key.len() != 32 || envelope.signature.len() != 64 {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        if envelope.node_id != Self::node_id_for_public_key(&envelope.public_key) {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        let now = chrono::Utc::now().timestamp();
        if (envelope.timestamp - now).abs() > MAX_TIMESTAMP_SKEW_SECS {
            return Err(xentrium_shared::XentriumError::InvalidSignature);
        }
        let mut pk_bytes = [0u8; 32];
        pk_bytes.copy_from_slice(&envelope.public_key);
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes)
            .map_err(|_| xentrium_shared::XentriumError::InvalidSignature)?;
        let signed = Self::envelope_signed_bytes(
            domain, &envelope.node_id, envelope.timestamp, envelope.nonce,
            &envelope.public_key, &envelope.payload);
        verify_signature(&verifying_key, &signed, &envelope.signature)?;
        Ok(envelope.payload.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identity_generation() {
        let id = NodeIdentity::generate();
        assert!(id.node_id.starts_with("xnt-"));
        assert_eq!(id.public_key_bytes().len(), 32);
    }

    #[test]
    fn test_identity_persistence() {
        let dir = std::env::temp_dir().join(format!("xnt_id_test_{}", std::process::id()));
        let path = dir.join("identity.key");
        let _ = std::fs::remove_file(&path);

        let id1 = NodeIdentity::load_or_create(&path).unwrap();
        let id1_node = id1.node_id.clone();
        let id1_pub = id1.public_key_bytes();

        // Reload - should be identical
        let id2 = NodeIdentity::load_or_create(&path).unwrap();
        assert_eq!(id1_node, id2.node_id);
        assert_eq!(id1_pub, id2.public_key_bytes());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_handshake_roundtrip() {
        let id = NodeIdentity::generate();
        let hs = id.create_handshake("127.0.0.1:30303");
        assert!(NodeIdentity::verify_handshake(&hs).is_ok());
    }

    #[test]
    fn test_handshake_rejection_bad_signature() {
        let id = NodeIdentity::generate();
        let mut hs = id.create_handshake("127.0.0.1:30303");
        hs.signature[0] ^= 0xFF; // corrupt signature
        assert!(NodeIdentity::verify_handshake(&hs).is_err());
    }

    #[test]
    fn test_handshake_rejection_malformed() {
        let id = NodeIdentity::generate();
        let hs = id.create_handshake("127.0.0.1:30303");

        // Empty node_id
        let mut bad = hs.clone();
        bad.node_id = String::new();
        assert!(NodeIdentity::verify_handshake(&bad).is_err());

        // Bad pubkey length
        let mut bad2 = hs.clone();
        bad2.public_key = vec![0u8; 16];
        assert!(NodeIdentity::verify_handshake(&bad2).is_err());

        // Bad signature length
        let mut bad3 = hs.clone();
        bad3.signature = vec![0u8; 10];
        assert!(NodeIdentity::verify_handshake(&bad3).is_err());

        // Stale timestamp
        let mut bad4 = hs.clone();
        bad4.timestamp = chrono::Utc::now().timestamp() - 200000;
        // re-sign won't help - signature covers timestamp, but tampering invalidates it anyway
        assert!(NodeIdentity::verify_handshake(&bad4).is_err());
    }

    #[test]
    fn test_announcement_verify() {
        let id = NodeIdentity::generate();
        let ann = id.sign_announcement("127.0.0.1:30303");
        assert!(NodeIdentity::verify_announcement(&ann).is_ok());

        // Tampered endpoint invalidates signature
        let mut bad = ann.clone();
        bad.endpoint = "6.6.6.6:1".to_string();
        assert!(NodeIdentity::verify_announcement(&bad).is_err());
    }

    #[test]
    fn test_node_id_binding() {
        let id = NodeIdentity::generate();
        let mut hs = id.create_handshake("127.0.0.1:30303");
        // Forge: claim a different node_id (signature no longer matches anyway,
        // but binding check must reject even before signature verification)
        hs.node_id = "xnt-forged".to_string();
        assert!(NodeIdentity::verify_handshake(&hs).is_err());

        // Announcement with mismatched node_id/pubkey also rejected
        let mut ann = id.sign_announcement("127.0.0.1:30303");
        ann.node_id = "xnt-forged".to_string();
        assert!(NodeIdentity::verify_announcement(&ann).is_err());
    }

    #[test]
    fn test_timestamp_window() {
        let id = NodeIdentity::generate();
        let mut hs = id.create_handshake("127.0.0.1:30303");
        // 10 minutes old — outside the 5-minute window (signature becomes invalid too,
        // but a fresh-sign check would also fail; verify rejects regardless)
        hs.timestamp = chrono::Utc::now().timestamp() - 600;
        assert!(NodeIdentity::verify_handshake(&hs).is_err());
    }

    #[test]
    fn test_replay_guard() {
        let mut guard = ReplayGuard::new(100);
        assert!(guard.check_and_insert("node-a", 42));
        assert!(!guard.check_and_insert("node-a", 42)); // replay
        assert!(guard.check_and_insert("node-a", 43)); // new nonce ok
        assert!(guard.check_and_insert("node-b", 42)); // different node ok
    }

    #[test]
    fn test_signed_envelope() {
        let id = NodeIdentity::generate();
        let inner = b"validator-payload".to_vec();
        let env = id.sign_envelope("test-domain", &inner);
        let out = NodeIdentity::verify_envelope(&env, "test-domain").unwrap();
        assert_eq!(out, inner);

        // Wrong domain rejected
        assert!(NodeIdentity::verify_envelope(&env, "other-domain").is_err());

        // Tampered payload rejected
        let mut bad = env.clone();
        bad.payload = b"evil".to_vec();
        assert!(NodeIdentity::verify_envelope(&bad, "test-domain").is_err());
    }
}
