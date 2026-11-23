use xentrium_network::{NodeIdentity, HandshakePayload, Message, MessageType};
use xentrium_node::{Node, NodeConfig};

fn cfg(node_id: &str, addr: &str, dir: &str, bootstrap: Vec<String>) -> NodeConfig {
    NodeConfig {
        node_id: node_id.into(), network_address: addr.into(),
        data_directory: dir.into(), bootstrap_peers: bootstrap,
        max_peers: 50, log_level: "info".into(),
        heartbeat_interval_secs: 30, peer_timeout_secs: 120,
        allowed_node_ids: vec![], ..NodeConfig::default()
    }
}

#[tokio::test]
async fn test_identity_generation_and_persistence() {
    let dir = std::env::temp_dir().join(format!("xnt_sec_{}", std::process::id()));
    let path = dir.join("node.identity");
    let _ = std::fs::remove_dir_all(&dir);

    let id1 = NodeIdentity::load_or_create(&path).unwrap();
    assert!(id1.node_id.starts_with("xnt-"));
    assert!(path.exists(), "identity file should be persisted");

    let id2 = NodeIdentity::load_or_create(&path).unwrap();
    assert_eq!(id1.node_id, id2.node_id, "identity must persist across loads");
    assert_eq!(id1.public_key_bytes(), id2.public_key_bytes());

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_handshake_success() {
    let id = NodeIdentity::generate();
    let hs = id.create_handshake("127.0.0.1:30303");
    assert!(NodeIdentity::verify_handshake(&hs).is_ok());

    // Round-trip through message serialization
    let payload = Message::serialize_handshake(&hs).unwrap();
    let decoded = Message::deserialize_handshake(&payload).unwrap();
    assert!(NodeIdentity::verify_handshake(&decoded).is_ok());
}

#[tokio::test]
async fn test_handshake_rejection_malformed() {
    let id = NodeIdentity::generate();
    let hs = id.create_handshake("127.0.0.1:30303");

    let mut bad_pub = hs.clone();
    bad_pub.public_key = vec![0u8; 8];
    assert!(NodeIdentity::verify_handshake(&bad_pub).is_err());

    let mut bad_sig = hs.clone();
    bad_sig.signature = vec![0u8; 4];
    assert!(NodeIdentity::verify_handshake(&bad_sig).is_err());

    let mut bad_node = hs.clone();
    bad_node.node_id = String::new();
    assert!(NodeIdentity::verify_handshake(&bad_node).is_err());

    let mut bad_ts = hs.clone();
    bad_ts.timestamp = 0;
    assert!(NodeIdentity::verify_handshake(&bad_ts).is_err());
}

#[tokio::test]
async fn test_invalid_signature_rejection() {
    let id = NodeIdentity::generate();
    let mut hs = id.create_handshake("127.0.0.1:30303");
    hs.signature[10] ^= 0xFF;
    assert!(NodeIdentity::verify_handshake(&hs).is_err());

    // Signature from a different key must fail
    let other = NodeIdentity::generate();
    let foreign = other.create_handshake("127.0.0.1:30303");
    // Tamper: swap node_id so signature no longer matches content
    let mut tampered = foreign.clone();
    tampered.node_id = "evil-node".to_string();
    assert!(NodeIdentity::verify_handshake(&tampered).is_err());
}

#[tokio::test]
async fn test_two_nodes_handshake_and_identity() {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d1 = format!("sec_n1_{}", ts);
    let d2 = format!("sec_n2_{}", ts);
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();

    let n1 = Node::new_with_storage(cfg("sec-node-1", "127.0.0.1:52101", &d1, vec![])).await.unwrap();
    n1.start().await.unwrap();

    let n2 = Node::new_with_storage(cfg("sec-node-2", "127.0.0.1:52102", &d2,
        vec!["127.0.0.1:52101".to_string()])).await.unwrap();
    n2.start().await.unwrap();

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    // Both nodes have persistent identities
    assert!(n1.identity().node_id.starts_with("xnt-"));
    assert!(n2.identity().node_id.starts_with("xnt-"));
    assert_ne!(n1.identity().node_id, n2.identity().node_id);

    // Identity files persisted on disk
    assert!(std::path::Path::new(&d1).join("node.identity").exists());
    assert!(std::path::Path::new(&d2).join("node.identity").exists());

    // Nodes connected (handshake exchanged without rejection)
    assert!(!n2.connections().await.is_empty());

    n1.stop().await.unwrap();
    n2.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&d1);
    let _ = std::fs::remove_dir_all(&d2);
}
