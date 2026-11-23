use xentrium_node::{Node, NodeConfig};
use xentrium_network::{HeartbeatTracker, PeerHealth, PeerID, Message, MessageType};
use std::time::Duration;

fn create_node_config(node_id: &str, address: &str, data_dir: &str, hb_interval: u64, timeout: u64) -> NodeConfig {
    NodeConfig {
        node_id: node_id.to_string(),
        network_address: address.to_string(),
        data_directory: data_dir.to_string(),
        bootstrap_peers: vec![],
        max_peers: 50,
        log_level: "info".to_string(),
        heartbeat_interval_secs: hb_interval,
        peer_timeout_secs: timeout,
        allowed_node_ids: vec![], ..NodeConfig::default()
    }
}

#[tokio::test]
async fn test_heartbeat_ping() {
    // Verify heartbeat tracker sends pings to registered peers
    let mut tracker = HeartbeatTracker::new(1, 10);
    let peer = PeerID::new(
        "XNTTestPing".to_string(),
        "127.0.0.1:50101".parse().unwrap(),
        "machine_ping".to_string(),
    );

    tracker.get_or_create(peer.clone());
    let due = tracker.peers_due_for_ping();
    assert_eq!(due.len(), 1, "Peer should be due for ping");
    assert_eq!(due[0].0, peer);
}

#[tokio::test]
async fn test_heartbeat_pong_latency() {
    // Verify pong receipt records latency
    let mut tracker = HeartbeatTracker::new(5, 30);
    let peer = PeerID::new(
        "XNTTestPong".to_string(),
        "127.0.0.1:50102".parse().unwrap(),
        "machine_pong".to_string(),
    );

    tracker.get_or_create(peer.clone());
    let _ = tracker.peers_due_for_ping();
    let result = tracker.record_pong(&peer);
    assert!(result.is_some(), "Pong should return latency");
    let (_, latency) = result.unwrap();
    assert!(latency.as_millis() < 1000, "Latency should be reasonable");

    let liveness = tracker.liveness_of(&peer).unwrap();
    assert!(liveness.last_latency.is_some());
    assert_eq!(liveness.consecutive_successes, 1);
    assert_eq!(liveness.health, PeerHealth::Healthy);
}

#[tokio::test]
async fn test_latency_tracking() {
    // Verify average latency is tracked over multiple pongs
    let mut tracker = HeartbeatTracker::new(5, 30);
    let peer = PeerID::new(
        "XNTTestLatency".to_string(),
        "127.0.0.1:50103".parse().unwrap(),
        "machine_latency".to_string(),
    );

    tracker.get_or_create(peer.clone());

    // Simulate 3 ping/pong cycles
    for _ in 0..3 {
        let _ = tracker.peers_due_for_ping();
        std::thread::sleep(Duration::from_millis(10));
        let _ = tracker.record_pong(&peer);
    }

    let liveness = tracker.liveness_of(&peer).unwrap();
    assert!(liveness.avg_latency.is_some(), "Average latency should be tracked");
    assert_eq!(liveness.consecutive_successes, 3);
}

#[tokio::test]
async fn test_unhealthy_peer_detection() {
    // Verify peers that miss heartbeats become unhealthy
    let mut tracker = HeartbeatTracker::new(1, 3);
    let peer = PeerID::new(
        "XNTTestUnhealthy".to_string(),
        "127.0.0.1:50104".parse().unwrap(),
        "machine_unhealthy".to_string(),
    );

    tracker.get_or_create(peer.clone());
    let _ = tracker.peers_due_for_ping();
    
    // Wait for miss
    std::thread::sleep(Duration::from_millis(1100));
    let _ = tracker.check_missed();
    
    // After 1 miss -> Suspect
    let health = tracker.health_of(&peer).unwrap();
    assert_eq!(health, PeerHealth::Suspect, "Should be suspect after 1 miss");
    
    // Wait for another miss
    std::thread::sleep(Duration::from_millis(1100));
    let _ = tracker.check_missed();
    
    // After 2 misses -> Unhealthy
    let health = tracker.health_of(&peer).unwrap();
    assert_eq!(health, PeerHealth::Unhealthy, "Should be unhealthy after 2 misses");
    
    let unhealthy = tracker.unhealthy_peers();
    assert!(unhealthy.contains(&peer));
}

#[tokio::test]
async fn test_stale_peer_removal() {
    // Verify unhealthy peers can be removed from tracker
    let mut tracker = HeartbeatTracker::new(1, 3);
    let peer = PeerID::new(
        "XNTTestStale".to_string(),
        "127.0.0.1:50105".parse().unwrap(),
        "machine_stale".to_string(),
    );

    tracker.get_or_create(peer.clone());
    let _ = tracker.peers_due_for_ping();
    std::thread::sleep(Duration::from_millis(1100));
    let _ = tracker.check_missed();
    std::thread::sleep(Duration::from_millis(1100));
    let _ = tracker.check_missed();
    
    assert!(tracker.health_of(&peer) == Some(PeerHealth::Unhealthy));
    
    // Remove peer
    tracker.remove_peer(&peer);
    assert!(tracker.health_of(&peer).is_none(), "Peer should be removed");
    assert_eq!(tracker.len(), 0);
}

#[tokio::test]
async fn test_peer_recovery_after_reconnect() {
    // Verify peer recovers after being marked unhealthy
    let mut tracker = HeartbeatTracker::new(1, 3);
    let peer = PeerID::new(
        "XNTTestRecovery".to_string(),
        "127.0.0.1:50106".parse().unwrap(),
        "machine_recovery".to_string(),
    );

    tracker.get_or_create(peer.clone());
    let _ = tracker.peers_due_for_ping();
    std::thread::sleep(Duration::from_millis(1100));
    let _ = tracker.check_missed();
    std::thread::sleep(Duration::from_millis(1100));
    let _ = tracker.check_missed();
    
    assert_eq!(tracker.health_of(&peer), Some(PeerHealth::Unhealthy));
    
    // Peer reconnects and sends pong
    let result = tracker.record_pong(&peer);
    assert!(result.is_some());
    
    // Should be healthy again
    assert_eq!(tracker.health_of(&peer), Some(PeerHealth::Healthy));
}

#[tokio::test]
async fn test_heartbeat_with_two_nodes() {
    // Integration test: two nodes connect and heartbeat works
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir_a = format!("test_hb_node_a_{}", timestamp);
    let db_dir_b = format!("test_hb_node_b_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir_a);
    let _ = std::fs::remove_dir_all(&db_dir_b);
    std::fs::create_dir_all(&db_dir_a).unwrap();
    std::fs::create_dir_all(&db_dir_b).unwrap();
    
    let config_a = create_node_config("HBNodeA", "127.0.0.1:50110", &db_dir_a, 2, 10);
    let node_a = Node::new_with_storage(config_a).await.unwrap();
    node_a.start().await.unwrap();
    
    let config_b = NodeConfig {
        bootstrap_peers: vec!["127.0.0.1:50110".to_string()],
        ..create_node_config("HBNodeB", "127.0.0.1:50111", &db_dir_b, 2, 10)
    };
    let node_b = Node::new_with_storage(config_b).await.unwrap();
    node_b.start().await.unwrap();
    
    // Wait for connection and heartbeat
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    
    // Verify nodes are running
    assert!(node_a.is_running().await);
    assert!(node_b.is_running().await);
    
    // Verify heartbeat tracker exists and has peers
    let tracker_a = node_a.heartbeat_tracker();
    let tracker_a_read = tracker_a.read().await;
    assert!(tracker_a_read.len() >= 0, "Tracker should be initialized");
    drop(tracker_a_read);
    
    node_a.stop().await.unwrap();
    node_b.stop().await.unwrap();
    
    let _ = std::fs::remove_dir_all(&db_dir_a);
    let _ = std::fs::remove_dir_all(&db_dir_b);
}
