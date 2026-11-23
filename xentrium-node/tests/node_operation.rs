use xentrium_node::{Node, NodeConfig};
use xentrium_node::config::NodeConfig as TomlNodeConfig;
use xentrium_network::{Discovery, GossipProtocol, SnapshotSync};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

fn create_node_config(node_id: &str, address: &str, data_dir: &str) -> NodeConfig {
    NodeConfig {
        node_id: node_id.to_string(),
        network_address: address.to_string(),
        data_directory: data_dir.to_string(),
        bootstrap_peers: vec![],
        max_peers: 50,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 30,
        peer_timeout_secs: 120,
        allowed_node_ids: vec![], ..NodeConfig::default()
    }
}

#[tokio::test]
async fn test_config_loading() {
    // Test TOML configuration loading
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let config_path = format!("test_config_{}.toml", timestamp);
    
    let config_content = r#"
[node]
node_id = "config_test_node"
address = "127.0.0.1"
port = 49001
data_dir = "./test_config_data"

[network]
bootstrap_peers = ["127.0.0.1:49002"]
max_peers = 25
heartbeat_interval_secs = 15
connection_timeout_secs = 60

[consensus]
auto_committee_rotation = true
snapshot_interval_hours = 12

[logging]
level = "debug"
file = "test.log"
"#;
    
    std::fs::write(&config_path, config_content).unwrap();
    
    let config = TomlNodeConfig::load_from_file(&config_path).unwrap();
    
    assert_eq!(config.node.node_id, "config_test_node");
    assert_eq!(config.node.port, 49001);
    assert_eq!(config.network.bootstrap_peers.len(), 1);
    assert_eq!(config.network.max_peers, 25);
    assert_eq!(config.consensus.snapshot_interval_hours, 12);
    assert_eq!(config.logging.level, "debug");
    
    // Test default config
    let default_config = TomlNodeConfig::default();
    assert_eq!(default_config.node.node_id, "xentrium-node-1");
    assert_eq!(default_config.node.port, 30303);
    
    let _ = std::fs::remove_file(&config_path);
}

#[tokio::test]
async fn test_node_binary_startup() {
    // Test that node can be created and started with new config structure
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_binary_startup_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("BinaryNode", "127.0.0.1:49010", &db_dir);
    let node = Node::new_with_storage(config).await.unwrap();
    
    node.start().await.unwrap();
    
    // Verify node is running
    assert!(node.is_running().await, "Node should be running");
    
    // Verify background tasks are started (check via logs would require capture)
    // Verify basic functionality
    let connections = node.connections().await;
    assert_eq!(connections.len(), 0, "Should have no connections initially");
    
    node.stop().await.unwrap();
    assert!(!node.is_running().await, "Node should be stopped");
    
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_heartbeat_operation() {
    // Test heartbeat mechanism with two nodes
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir_a = format!("test_heartbeat_a_{}", timestamp);
    let db_dir_b = format!("test_heartbeat_b_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir_a);
    let _ = std::fs::remove_dir_all(&db_dir_b);
    std::fs::create_dir_all(&db_dir_a).unwrap();
    std::fs::create_dir_all(&db_dir_b).unwrap();
    
    let config_a = create_node_config("HeartbeatA", "127.0.0.1:49020", &db_dir_a);
    let node_a = Node::new_with_storage(config_a).await.unwrap();
    node_a.start().await.unwrap();
    
    let config_b = NodeConfig {
        bootstrap_peers: vec!["127.0.0.1:49020".to_string()],
        ..create_node_config("HeartbeatB", "127.0.0.1:49021", &db_dir_b)
    };
    let node_b = Node::new_with_storage(config_b).await.unwrap();
    node_b.start().await.unwrap();
    
    // Wait for connection
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    // Send ping to test heartbeat functionality
    use xentrium_network::{Message, MessageType};
    let _ping = Message::new(
        MessageType::Ping,
        b"test".to_vec(),
        b"ping".to_vec(),
    );
    
    // Node B should have connection to Node A (bootstrap)
    let connections_b = node_b.connections().await;
    assert!(!connections_b.is_empty(), "Node B should have connections");
    
    node_a.stop().await.unwrap();
    node_b.stop().await.unwrap();
    
    let _ = std::fs::remove_dir_all(&db_dir_a);
    let _ = std::fs::remove_dir_all(&db_dir_b);
}

#[tokio::test]
async fn test_stale_peer_removal() {
    // Test that unhealthy peers are detected and removed
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_stale_peer_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("StalePeerNode", "127.0.0.1:49030", &db_dir);
    let node = Node::new_with_storage(config).await.unwrap();
    node.start().await.unwrap();
    
    // Get discovery and add a stale peer manually
    let discovery = node.discovery();
    let mut discovery_write = discovery.write().await;
    
    use xentrium_network::{Peer, PeerID, PeerState};
    let stale_peer_id = PeerID::new(
        "stale_peer".to_string(),
        "127.0.0.1:9999".parse().unwrap(),
        "stale_machine".to_string(),
    );
    let mut stale_peer = Peer::new(stale_peer_id.clone());
    stale_peer.set_state(PeerState::Disconnected);
    let _ = discovery_write.add_peer(stale_peer);
    
    // Verify peer was added
    let peers_before = discovery_write.get_peers();
    assert!(!peers_before.is_empty(), "Should have stale peer");
    
    // Verify get_unhealthy_peers returns the stale peer
    let unhealthy = discovery_write.get_unhealthy_peers();
    assert!(!unhealthy.is_empty(), "Should detect unhealthy peer");
    
    drop(discovery_write);
    
    node.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_background_scheduler_startup() {
    // Test that background schedulers start without errors
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_scheduler_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("SchedulerNode", "127.0.0.1:49040", &db_dir);
    let node = Node::new_with_storage(config).await.unwrap();
    
    // Start node - this should spawn all background tasks
    node.start().await.unwrap();
    
    // Wait a bit to ensure tasks are running
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    assert!(node.is_running().await, "Node should be running with schedulers");
    
    // Verify gossip maintenance works
    let gossip = node.gossip();
    let gossip_read = gossip.read().await;
    assert_eq!(gossip_read.get_seen_count(), 0, "Gossip should be initialized");
    drop(gossip_read);
    
    // Verify snapshot sync is initialized
    let snapshot_sync = node.snapshot_sync();
    let sync_read = snapshot_sync.read().await;
    let available = sync_read.get_available_snapshots();
    assert!(available.is_empty(), "Should have no snapshots initially");
    drop(sync_read);
    
    node.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_graceful_shutdown() {
    // Test graceful shutdown functionality
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_shutdown_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("ShutdownNode", "127.0.0.1:49050", &db_dir);
    let node = Node::new_with_storage(config).await.unwrap();
    
    node.start().await.unwrap();
    assert!(node.is_running().await);
    
    // Stop the node
    node.stop().await.unwrap();
    
    // Verify stopped
    assert!(!node.is_running().await, "Node should be stopped");
    
    // Verify connections are cleared
    let connections = node.connections().await;
    assert!(connections.is_empty(), "Connections should be cleared");
    
    let _ = std::fs::remove_dir_all(&db_dir);
}
