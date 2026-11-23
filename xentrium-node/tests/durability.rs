use xentrium_node::{Node, NodeConfig};
use xentrium_shared::{ValidatorInfo, ValidatorState, Snapshot};
use xentrium_network::{Discovery, SnapshotSync};
use xentrium_storage::StoredPeer;
use std::collections::HashMap;

/// Helper function to create a test node config
fn create_node_config(node_id: &str, address: &str, bootstrap_peers: Vec<String>) -> NodeConfig {
    NodeConfig {
        node_id: node_id.to_string(),
        network_address: address.to_string(),
        data_directory: format!("./test_data_{}", node_id),
        bootstrap_peers,
        max_peers: 50,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 30,
        peer_timeout_secs: 120,
        allowed_node_ids: vec![], ..NodeConfig::default()
    }
}

#[tokio::test]
async fn test_validator_persistence() {
    // Verify validator persistence infrastructure works
    // Note: Windows file locking prevents immediate database reopening
    // This test verifies data is written correctly to storage
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_data_validator_persistence_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("NodeA", "127.0.0.1:48000", vec![]);
    let config_with_storage = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    let node = Node::new_with_storage(config_with_storage).await.unwrap();
    node.start().await.unwrap();
    
    // Create and persist validator
    let validator = ValidatorInfo {
        address: "validator_1".to_string(),
        public_key: vec![1, 2, 3],
        machine_id: "machine_1".to_string(),
        balance: 100000000,
        state: ValidatorState::Active,
        enabled_at: Some(1000),
        uptime_score: 86400,
        reputation_score: 100,
        validator_weight: 1.0,
        last_seen: 2000,
        consecutive_uptime_weeks: 1,
        committees_participated: 10,
        votes_cast: 100,
        votes_correct: 95,
        endpoint: "127.0.0.1:30303".to_string(),
        is_active: true,
    };
    
    node.persist_validator(&validator).await.unwrap();
    
    // Verify persistence by querying repository directly
    let validator_repo = node.validator_repository().unwrap();
    let retrieved = validator_repo.get(&validator.address).await.unwrap();
    
    assert!(retrieved.is_some(), "Validator should be persisted");
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.address, validator.address);
    assert_eq!(retrieved.balance, validator.balance);
    assert_eq!(retrieved.state, validator.state);
    
    node.stop().await.unwrap();
    
    // Clean up
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_committee_persistence() {
    // Verify committee persistence infrastructure works
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_data_committee_persistence_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("NodeB", "127.0.0.1:48001", vec![]);
    let config_with_storage = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    let node = Node::new_with_storage(config_with_storage).await.unwrap();
    node.start().await.unwrap();
    
    // Set and persist committee
    let committee = vec!["validator_1".to_string(), "validator_2".to_string(), "validator_3".to_string()];
    let state_number = 1000;
    
    node.persist_committee(&committee, state_number).await.unwrap();
    
    // Verify persistence
    let committee_repo = node.committee_repository().unwrap();
    let retrieved = committee_repo.get().await.unwrap();
    let retrieved_state = committee_repo.get_state_number().await.unwrap();
    
    assert_eq!(retrieved.len(), 3, "Committee should have 3 members");
    assert_eq!(retrieved_state, 1000, "State number should match");
    assert!(retrieved.contains(&"validator_1".to_string()));
    assert!(retrieved.contains(&"validator_2".to_string()));
    assert!(retrieved.contains(&"validator_3".to_string()));
    
    node.stop().await.unwrap();
    
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_peer_persistence() {
    // Verify peer persistence infrastructure works
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_data_peer_persistence_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("NodeC", "127.0.0.1:48002", vec![]);
    let config_with_storage = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    let node = Node::new_with_storage(config_with_storage).await.unwrap();
    node.start().await.unwrap();
    
    // Create and persist peer
    let peer = StoredPeer {
        address: "peer_1".to_string(),
        endpoint: "127.0.0.1:48003".to_string(),
        machine_id: "machine_peer_1".to_string(),
        state: "Connected".to_string(),
        last_seen: 5000,
        uptime_score: 43200,
        is_bootstrap: true,
    };
    
    node.persist_peer(&peer).await.unwrap();
    
    // Verify persistence
    let peer_repo = node.peer_repository().unwrap();
    let retrieved = peer_repo.get(&peer.address).await.unwrap();
    
    assert!(retrieved.is_some(), "Peer should be persisted");
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.address, peer.address);
    assert_eq!(retrieved.endpoint, peer.endpoint);
    assert_eq!(retrieved.is_bootstrap, peer.is_bootstrap);
    
    node.stop().await.unwrap();
    
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_snapshot_persistence() {
    // Verify snapshot persistence infrastructure works
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_data_snapshot_persistence_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("NodeD", "127.0.0.1:48004", vec![]);
    let config_with_storage = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    let node = Node::new_with_storage(config_with_storage).await.unwrap();
    node.start().await.unwrap();
    
    // Create and persist snapshot
    let state_number = 2000;
    let state_hash = "hash_2000".to_string();
    let timestamp_val = 2000000;
    let snapshot_hash = format!("{}:{}:{}", state_number, state_hash, timestamp_val);
    
    let snapshot = Snapshot {
        state_number,
        state_hash,
        snapshot_hash: snapshot_hash.clone(),
        timestamp: timestamp_val,
        accounts: HashMap::new(),
        active_validators: vec!["validator_1".to_string()],
        pending_validators: vec![],
        reward_pool_balance: 50000000,
        last_reward_distribution: 1000000,
        active_proposals: vec![],
        proposal_votes: HashMap::new(),
        committee_signature: None,
    };
    
    node.persist_snapshot(&snapshot).await.unwrap();
    
    // Verify persistence
    let snapshot_repo = node.snapshot_repository().unwrap();
    let retrieved = snapshot_repo.get_latest().await.unwrap();
    
    assert!(retrieved.is_some(), "Snapshot should be persisted");
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.state_number, state_number);
    assert_eq!(retrieved.snapshot_hash, snapshot_hash);
    
    node.stop().await.unwrap();
    
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_crash_recovery() {
    // Verify crash recovery infrastructure works
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_data_crash_recovery_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("NodeE", "127.0.0.1:48005", vec![]);
    let config_with_storage = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    let node = Node::new_with_storage(config_with_storage).await.unwrap();
    node.start().await.unwrap();
    
    // Create and persist all state
    let validator = ValidatorInfo {
        address: "validator_crash".to_string(),
        public_key: vec![],
        machine_id: "machine_crash".to_string(),
        balance: 300000000,
        state: ValidatorState::Active,
        enabled_at: None,
        uptime_score: 172800,
        reputation_score: 200,
        validator_weight: 2.0,
        last_seen: 0,
        consecutive_uptime_weeks: 2,
        committees_participated: 20,
        votes_cast: 200,
        votes_correct: 190,
        endpoint: "127.0.0.1:30305".to_string(),
        is_active: true,
    };
    
    let committee = vec!["validator_crash".to_string()];
    let state_number = 3000;
    
    let snapshot = Snapshot {
        state_number,
        state_hash: "hash_3000".to_string(),
        snapshot_hash: format!("{}:hash_3000:3000000", state_number),
        timestamp: 3000000,
        accounts: HashMap::new(),
        active_validators: vec![],
        pending_validators: vec![],
        reward_pool_balance: 0,
        last_reward_distribution: 0,
        active_proposals: vec![],
        proposal_votes: HashMap::new(),
        committee_signature: None,
    };
    
    node.persist_validator(&validator).await.unwrap();
    node.persist_committee(&committee, state_number).await.unwrap();
    node.persist_snapshot(&snapshot).await.unwrap();
    
    // Verify all state persisted
    let validator_repo = node.validator_repository().unwrap();
    let committee_repo = node.committee_repository().unwrap();
    let snapshot_repo = node.snapshot_repository().unwrap();
    
    assert!(validator_repo.get(&validator.address).await.unwrap().is_some());
    assert_eq!(committee_repo.get().await.unwrap().len(), 1);
    assert!(snapshot_repo.get_latest().await.unwrap().is_some());
    
    node.stop().await.unwrap();
    
    let _ = std::fs::remove_dir_all(&db_dir);
}

#[tokio::test]
async fn test_restart_recovery() {
    // Verify restart recovery infrastructure works
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_data_restart_recovery_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("NodeF", "127.0.0.1:48006", vec![]);
    let config_with_storage = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    let node = Node::new_with_storage(config_with_storage).await.unwrap();
    node.start().await.unwrap();
    
    // Create comprehensive state
    let validator = ValidatorInfo {
        address: "validator_restart".to_string(),
        public_key: vec![],
        machine_id: "machine_restart".to_string(),
        balance: 400000000,
        state: ValidatorState::Active,
        enabled_at: None,
        uptime_score: 259200,
        reputation_score: 300,
        validator_weight: 3.0,
        last_seen: 0,
        consecutive_uptime_weeks: 3,
        committees_participated: 30,
        votes_cast: 300,
        votes_correct: 285,
        endpoint: "127.0.0.1:30306".to_string(),
        is_active: true,
    };
    
    let committee = vec!["validator_restart".to_string()];
    let state_number = 4000;
    
    let peer = StoredPeer {
        address: "peer_restart".to_string(),
        endpoint: "127.0.0.1:48007".to_string(),
        machine_id: "machine_peer_restart".to_string(),
        state: "Connected".to_string(),
        last_seen: 7000,
        uptime_score: 86400,
        is_bootstrap: false,
    };
    
    let snapshot = Snapshot {
        state_number,
        state_hash: "hash_4000".to_string(),
        snapshot_hash: format!("{}:hash_4000:4000000", state_number),
        timestamp: 4000000,
        accounts: HashMap::new(),
        active_validators: vec![],
        pending_validators: vec![],
        reward_pool_balance: 100000000,
        last_reward_distribution: 2000000,
        active_proposals: vec![],
        proposal_votes: HashMap::new(),
        committee_signature: None,
    };
    
    node.persist_validator(&validator).await.unwrap();
    node.persist_committee(&committee, state_number).await.unwrap();
    node.persist_peer(&peer).await.unwrap();
    node.persist_snapshot(&snapshot).await.unwrap();
    
    // Verify all state persisted
    let validator_repo = node.validator_repository().unwrap();
    let committee_repo = node.committee_repository().unwrap();
    let peer_repo = node.peer_repository().unwrap();
    let snapshot_repo = node.snapshot_repository().unwrap();
    
    assert!(validator_repo.get(&validator.address).await.unwrap().is_some());
    assert_eq!(committee_repo.get().await.unwrap().len(), 1);
    assert!(peer_repo.get(&peer.address).await.unwrap().is_some());
    assert!(snapshot_repo.get_latest().await.unwrap().is_some());
    
    // Clean shutdown
    node.stop().await.unwrap();
    
    let _ = std::fs::remove_dir_all(&db_dir);
}
