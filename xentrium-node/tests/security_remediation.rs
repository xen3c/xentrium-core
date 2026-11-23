use xentrium_node::{Node, NodeConfig};
use xentrium_network::{
    Message, MessageType, NodeIdentity, ReplayGuard,
};
use xentrium_shared::ValidatorInfo;
use xentrium_shared::ValidatorState;
use std::net::SocketAddr;
use std::time::Duration;

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn node_config(node_id: &str, port: u16, bootstrap: Vec<String>, allowed: Vec<String>) -> NodeConfig {
    NodeConfig {
        node_id: node_id.to_string(),
        network_address: format!("127.0.0.1:{}", port),
        data_directory: format!("./data_sec/{}", node_id),
        bootstrap_peers: bootstrap,
        max_peers: 50,
        log_level: "warn".to_string(),
        heartbeat_interval_secs: 30,
        peer_timeout_secs: 120,
        allowed_node_ids: allowed, ..NodeConfig::default()
    }
}

fn validator(address: &str) -> ValidatorInfo {
    ValidatorInfo {
        address: address.to_string(),
        public_key: vec![],
        machine_id: String::new(),
        balance: 100000000,
        state: ValidatorState::Active,
        enabled_at: None,
        uptime_score: 86400,
        reputation_score: 100,
        validator_weight: 0.0,
        last_seen: 0,
        consecutive_uptime_weeks: 0,
        committees_participated: 0,
        votes_cast: 0,
        votes_correct: 0,
        endpoint: String::new(),
        is_active: true,
    }
}

async fn two_nodes(tag: &str, port_a: u16, port_b: u16) -> (Node, Node) {
    let a = Node::new(node_config(&format!("{}A", tag), port_a, vec![], vec![])).unwrap();
    a.start().await.unwrap();
    let b = Node::new(node_config(
        &format!("{}B", tag), port_b, vec![format!("127.0.0.1:{}", port_a)], vec![],
    )).unwrap();
    b.start().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    (a, b)
}

// ---------- C1: node_id bound to public key ----------

#[test]
fn c1_node_id_must_match_public_key() {
    let id = NodeIdentity::generate();
    let mut hs = id.create_handshake("127.0.0.1:1");
    // Forge node_id unrelated to the key — must be rejected even with valid structure
    hs.node_id = "xnt-0000000000000000".to_string();
    assert!(NodeIdentity::verify_handshake(&hs).is_err());

    // Announcement with swapped node_id also rejected
    let mut ann = id.sign_announcement("127.0.0.1:1");
    ann.node_id = "xnt-0000000000000000".to_string();
    assert!(NodeIdentity::verify_announcement(&ann).is_err());

    // Envelope with forged node_id rejected
    let env = id.sign_envelope("d", b"x");
    let mut bad_env = env.clone();
    bad_env.node_id = "xnt-0000000000000000".to_string();
    assert!(NodeIdentity::verify_envelope(&bad_env, "d").is_err());
}

// ---------- C2: maximum message size ----------

#[tokio::test]
async fn c2_oversized_frame_rejected() {
    use tokio::io::AsyncWriteExt;
    let mut server = xentrium_network::TcpTransport::new();
    server.start_listener("127.0.0.1:0".parse().unwrap()).await.unwrap();
    let addr: SocketAddr = server.local_address().unwrap();

    // Raw TCP client sends a 4-byte length prefix claiming > MAX_MESSAGE_SIZE
    let mut raw = tokio::net::TcpStream::connect(addr).await.unwrap();
    let huge = (xentrium_network::MAX_MESSAGE_SIZE as u32) + 1;
    raw.write_all(&huge.to_be_bytes()).await.unwrap();

    let mut conn = server.accept().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(3), conn.receive()).await;
    match result {
        Ok(Err(_)) => {} // expected rejection
        Ok(Ok(_)) => panic!("oversized frame must be rejected"),
        Err(_) => panic!("receive should error immediately, not hang"),
    }
}

// ---------- C3: signed validator registration & committee selection ----------

#[tokio::test]
async fn c3_signed_validator_registration_accepted() {
    let (a, b) = two_nodes("c3pos", 45201, 45202).await;

    let v = validator("val-c3");
    a.broadcast_validator_registration(v.clone()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;

    let vm_arc = b.validator_manager();
    let vm = vm_arc.read().await;
    let found = vm.get_pending_validators().iter().any(|x| x.address == "val-c3")
        || vm.get_active_validators().iter().any(|x| x.address == "val-c3");
    assert!(found, "signed validator registration should be processed");

    a.stop().await.unwrap();
    b.stop().await.unwrap();
}

#[tokio::test]
async fn c3_unsigned_validator_registration_rejected() {
    let (a, b) = two_nodes("c3neg", 45211, 45212).await;

    // Forge an UNSIGNED (old-format) ValidatorRegistration payload
    let inner = Message::serialize_validator_registration(&validator("val-evil")).unwrap();
    let forged = Message::new(MessageType::ValidatorRegistration, b"attacker".to_vec(), inner);
    a.broadcast_message(forged).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;

    let vm_arc = b.validator_manager();
    let vm = vm_arc.read().await;
    let found = vm.get_pending_validators().iter().any(|x| x.address == "val-evil")
        || vm.get_active_validators().iter().any(|x| x.address == "val-evil");
    assert!(!found, "unsigned validator registration must be rejected");

    a.stop().await.unwrap();
    b.stop().await.unwrap();
}

#[tokio::test]
async fn c3_wrong_identity_envelope_rejected() {
    let (a, b) = two_nodes("c3id", 45221, 45222).await;

    // Node A sends an envelope signed by a DIFFERENT identity (C) over the A->B connection.
    // B must reject: envelope signer != authenticated connection peer.
    let rogue = NodeIdentity::generate();
    let inner = Message::serialize_validator_registration(&validator("val-rogue")).unwrap();
    let env = rogue.sign_envelope("xnt-validator-registration", &inner);
    let payload = Message::serialize_signed_envelope(&env).unwrap();
    let msg = Message::new(MessageType::ValidatorRegistration, b"rogue".to_vec(), payload);
    a.broadcast_message(msg).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;

    let vm_arc = b.validator_manager();
    let vm = vm_arc.read().await;
    let found = vm.get_pending_validators().iter().any(|x| x.address == "val-rogue")
        || vm.get_active_validators().iter().any(|x| x.address == "val-rogue");
    assert!(!found, "envelope from non-connection identity must be rejected");

    a.stop().await.unwrap();
    b.stop().await.unwrap();
}

// ---------- H1: peer allowlist ----------

#[tokio::test]
async fn h1_allowlist_rejects_unknown_peer() {
    // Node B first (to learn its identity node_id)
    let b = Node::new(node_config("h1B", 45231, vec![], vec![])).unwrap();
    b.start().await.unwrap();
    let b_node_id = b.identity().node_id.clone();

    // Node A allows ONLY B's identity
    let a = Node::new(node_config("h1A", 45230, vec![], vec![b_node_id.clone()])).unwrap();
    a.start().await.unwrap();

    // Rogue node C (not allowlisted) connects to A
    let c = Node::new(node_config(
        "h1C", 45232, vec!["127.0.0.1:45230".to_string()], vec![],
    )).unwrap();
    c.start().await.unwrap();
    tokio::time::sleep(Duration::from_millis(2000)).await;

    // A must not retain a connection to C
    let a_conns = a.connections().await;
    assert!(a_conns.is_empty(), "non-allowlisted peer must be disconnected");

    a.stop().await.unwrap();
    b.stop().await.unwrap();
    c.stop().await.unwrap();
}

#[tokio::test]
async fn h1_allowlist_accepts_listed_peer() {
    // Determine B's identity before configuring A's allowlist
    let probe = Node::new(node_config("h1probe", 45241, vec![], vec![])).unwrap();
    let allowed_id = probe.identity().node_id.clone();
    probe.stop().await.unwrap();

    // Recreate the same identity? Node::new loads persisted identity from data dir —
    // the probe wrote ./data_sec/h1probe/node.identity, so a new node with same dir reuses it.
    let a = Node::new(node_config("h1A2", 45240, vec![], vec![allowed_id.clone()])).unwrap();
    a.start().await.unwrap();

    let b = Node::new(node_config("h1probe", 45242, vec!["127.0.0.1:45240".to_string()], vec![])).unwrap();
    b.start().await.unwrap();
    tokio::time::sleep(Duration::from_millis(2000)).await;

    let a_conns = a.connections().await;
    assert!(!a_conns.is_empty(), "allowlisted peer should remain connected");

    a.stop().await.unwrap();
    b.stop().await.unwrap();
}

// ---------- H2: signed announcements in PeerList gossip ----------

#[tokio::test]
async fn h2_announcement_sign_verify_tamper_replay() {
    let id = NodeIdentity::generate();
    let ann = id.sign_announcement("127.0.0.1:9999");

    // Valid
    assert!(NodeIdentity::verify_announcement(&ann).is_ok());

    // Tampered endpoint rejected
    let mut bad = ann.clone();
    bad.endpoint = "10.0.0.1:1".to_string();
    assert!(NodeIdentity::verify_announcement(&bad).is_err());

    // Replay: same (node_id, nonce) rejected by guard
    let mut guard = ReplayGuard::new(100);
    assert!(guard.check_and_insert(&ann.node_id, ann.nonce));
    assert!(!guard.check_and_insert(&ann.node_id, ann.nonce));
}

#[tokio::test]
async fn h2_peerlist_gossip_signed_announcements() {
    let (a, b) = two_nodes("h2", 45251, 45252).await;

    // A gossips its known announcements to B
    a.broadcast_peer_list().await.unwrap();
    tokio::time::sleep(Duration::from_millis(1000)).await;

    // B should have learned at least A's announcement (own + relayed)
    assert!(b.known_announcement_count().await >= 1,
        "B should cache verified signed announcements");

    a.stop().await.unwrap();
    b.stop().await.unwrap();
}

#[tokio::test]
async fn h2_forged_announcement_rejected() {
    let (a, b) = two_nodes("h2f", 45261, 45262).await;

    let before = b.known_announcement_count().await;

    // Forge an announcement with mismatched node_id (invalid signature binding)
    let rogue = NodeIdentity::generate();
    let mut ann = rogue.sign_announcement("127.0.0.1:1");
    ann.node_id = "xnt-forged".to_string();
    let payload = Message::serialize_announcements(&[ann]).unwrap();
    let msg = Message::new(MessageType::PeerList, b"evil".to_vec(), payload);
    a.broadcast_message(msg).await.unwrap();
    tokio::time::sleep(Duration::from_millis(800)).await;

    assert_eq!(b.known_announcement_count().await, before,
        "forged announcement must not be cached");

    a.stop().await.unwrap();
    b.stop().await.unwrap();
}

// ---------- H3+H4: replay protection & timestamp tolerance ----------

#[test]
fn h3_h4_freshness_and_replay() {
    let id = NodeIdentity::generate();

    // Timestamp beyond 5-minute window rejected
    let mut hs = id.create_handshake("127.0.0.1:1");
    hs.timestamp = now_ts() - 600;
    assert!(NodeIdentity::verify_handshake(&hs).is_err());

    // Fresh handshake passes structural+signature checks
    let ok = id.create_handshake("127.0.0.1:1");
    assert!(NodeIdentity::verify_handshake(&ok).is_ok());

    // Replay guard: duplicate (node_id, nonce) rejected, new nonce accepted
    let mut guard = ReplayGuard::new(100);
    assert!(guard.check_and_insert(&ok.node_id, ok.nonce));
    assert!(!guard.check_and_insert(&ok.node_id, ok.nonce));

    // Stale announcement rejected
    let mut ann = id.sign_announcement("127.0.0.1:1");
    ann.timestamp = now_ts() - 600;
    assert!(NodeIdentity::verify_announcement(&ann).is_err());
}
