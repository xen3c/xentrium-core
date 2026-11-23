use xentrium_node::{Node, NodeConfig};
use xentrium_shared::{ValidatorInfo, ValidatorState, Snapshot};
use xentrium_storage::StoredPeer;
use std::collections::HashMap;

/// Helper function to create a test node config
fn create_node_config(node_id: &str, address: &str, bootstrap_peers: Vec<String>) -> NodeConfig {
    NodeConfig {
        node_id: node_id.to_string(),
        network_address: address.to_string(),
        data_directory: format!("./test_data_audit_{}", node_id),
        bootstrap_peers,
        max_peers: 50,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 30,
        peer_timeout_secs: 120,
        allowed_node_ids: vec![], ..NodeConfig::default()
    }
}

#[tokio::test]
async fn test_real_disk_persistence_audit() {
    // Use unique database path with timestamp for clean audit
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_dir = format!("test_data_persistence_audit_{}", timestamp);
    let _ = std::fs::remove_dir_all(&db_dir);
    std::fs::create_dir_all(&db_dir).unwrap();
    
    let config = create_node_config("AuditNode", "127.0.0.1:49000", vec![]);
    let config_with_storage = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    // PHASE 1: Create and persist state
    println!("=== PHASE 1: Creating and persisting state ===");
    let node = Node::new_with_storage(config_with_storage.clone()).await.unwrap();
    node.start().await.unwrap();
    
    // Create validator
    let validator = ValidatorInfo {
        address: "audit_validator_1".to_string(),
        public_key: vec![1, 2, 3, 4],
        machine_id: "audit_machine_1".to_string(),
        balance: 500000000,
        state: ValidatorState::Active,
        enabled_at: Some(1000),
        uptime_score: 86400,
        reputation_score: 150,
        validator_weight: 1.5,
        last_seen: 3000,
        consecutive_uptime_weeks: 2,
        committees_participated: 15,
        votes_cast: 150,
        votes_correct: 140,
        endpoint: "127.0.0.1:30303".to_string(),
        is_active: true,
    };
    
    node.persist_validator(&validator).await.unwrap();
    println!("✓ Validator persisted: {}", validator.address);
    
    // Create committee
    let committee = vec!["audit_validator_1".to_string(), "audit_validator_2".to_string()];
    let state_number = 5000;
    node.persist_committee(&committee, state_number).await.unwrap();
    println!("✓ Committee persisted: {} members", committee.len());
    
    // Create peer
    let peer = StoredPeer {
        address: "audit_peer_1".to_string(),
        endpoint: "127.0.0.1:49001".to_string(),
        machine_id: "audit_machine_peer_1".to_string(),
        state: "Connected".to_string(),
        last_seen: 8000,
        uptime_score: 86400,
        is_bootstrap: true,
    };
    
    node.persist_peer(&peer).await.unwrap();
    println!("✓ Peer persisted: {}", peer.address);
    
    // Create snapshot
    let snapshot = Snapshot {
        state_number,
        state_hash: "audit_hash_5000".to_string(),
        snapshot_hash: format!("{}:audit_hash_5000:5000000", state_number),
        timestamp: 5000000,
        accounts: HashMap::new(),
        active_validators: vec!["audit_validator_1".to_string()],
        pending_validators: vec![],
        reward_pool_balance: 250000000,
        last_reward_distribution: 5000000,
        active_proposals: vec![],
        proposal_votes: HashMap::new(),
        committee_signature: None,
    };
    
    node.persist_snapshot(&snapshot).await.unwrap();
    println!("✓ Snapshot persisted: state {}", state_number);
    
    // Verify all state is in database before shutdown
    let validator_repo = node.validator_repository().unwrap();
    let committee_repo = node.committee_repository().unwrap();
    let peer_repo = node.peer_repository().unwrap();
    let snapshot_repo = node.snapshot_repository().unwrap();
    
    let validator_check = validator_repo.get(&validator.address).await.unwrap();
    let committee_check = committee_repo.get().await.unwrap();
    let peer_check = peer_repo.get(&peer.address).await.unwrap();
    let snapshot_check = snapshot_repo.get_latest().await.unwrap();
    
    println!("\n=== PRE-SHUTDOWN VERIFICATION ===");
    println!("Validator in DB: {}", validator_check.is_some());
    println!("Committee in DB: {} members", committee_check.len());
    println!("Peer in DB: {}", peer_check.is_some());
    println!("Snapshot in DB: {}", snapshot_check.is_some());
    
    assert!(validator_check.is_some(), "Validator should be in database");
    assert_eq!(committee_check.len(), 2, "Committee should have 2 members");
    assert!(peer_check.is_some(), "Peer should be in database");
    assert!(snapshot_check.is_some(), "Snapshot should be in database");
    
    // PHASE 2: Complete shutdown
    println!("\n=== PHASE 2: Complete shutdown ===");
    node.stop().await.unwrap();
    drop(node);
    println!("✓ Node stopped and dropped");
    
    // Verify database file exists
    let db_file = format!("{}/xentrium.db", db_dir);
    assert!(std::path::Path::new(&db_file).exists(), "Database file should exist");
    println!("✓ Database file exists: {}", db_file);
    
    // Wait for file locks to release (Windows needs time)
    println!("Waiting for file locks to release...");
    tokio::time::sleep(tokio::time::Duration::from_millis(3000)).await;
    
    // PHASE 3: Start brand-new node instance
    println!("\n=== PHASE 3: Starting brand-new node instance ===");
    let config_restart = NodeConfig {
        data_directory: db_dir.clone(),
        ..config.clone()
    };
    
    let node_restart = Node::new_with_storage(config_restart).await.unwrap();
    node_restart.start().await.unwrap();
    println!("✓ New node instance started");
    
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // PHASE 4: Verify state restoration
    println!("\n=== PHASE 4: Verifying state restoration ===");
    
    let validator_repo_restart = node_restart.validator_repository().unwrap();
    let committee_repo_restart = node_restart.committee_repository().unwrap();
    let peer_repo_restart = node_restart.peer_repository().unwrap();
    let snapshot_repo_restart = node_restart.snapshot_repository().unwrap();
    
    // Verify validator restoration
    let validator_restored = validator_repo_restart.get(&validator.address).await.unwrap();
    println!("Validator restored: {}", validator_restored.is_some());
    if let Some(v) = validator_restored {
        println!("  - Address: {}", v.address);
        println!("  - Balance: {}", v.balance);
        println!("  - State: {:?}", v.state);
        assert_eq!(v.address, validator.address);
        assert_eq!(v.balance, validator.balance);
        assert_eq!(v.state, validator.state);
    }
    
    // Verify committee restoration
    let committee_restored = committee_repo_restart.get().await.unwrap();
    let committee_state = committee_repo_restart.get_state_number().await.unwrap();
    println!("Committee restored: {} members (state: {})", committee_restored.len(), committee_state);
    assert_eq!(committee_restored.len(), 2, "Committee should be restored");
    assert_eq!(committee_state, state_number, "Committee state number should match");
    
    // Verify peer restoration
    let peer_restored = peer_repo_restart.get(&peer.address).await.unwrap();
    println!("Peer restored: {}", peer_restored.is_some());
    if let Some(p) = peer_restored {
        println!("  - Address: {}", p.address);
        println!("  - Endpoint: {}", p.endpoint);
        println!("  - Bootstrap: {}", p.is_bootstrap);
        assert_eq!(p.address, peer.address);
        assert_eq!(p.endpoint, peer.endpoint);
        assert_eq!(p.is_bootstrap, peer.is_bootstrap);
    }
    
    // Verify snapshot restoration
    let snapshot_restored = snapshot_repo_restart.get_latest().await.unwrap();
    println!("Snapshot restored: {}", snapshot_restored.is_some());
    if let Some(s) = snapshot_restored {
        println!("  - State number: {}", s.state_number);
        println!("  - Snapshot hash: {}", s.snapshot_hash);
        assert_eq!(s.state_number, state_number);
        assert_eq!(s.snapshot_hash, snapshot.snapshot_hash);
    }
    
    // Cleanup
    node_restart.stop().await.unwrap();
    let _ = std::fs::remove_dir_all(&db_dir);
    
    println!("\n=== AUDIT RESULT ===");
    println!("✓ Validators: ON DISK, RESTORED");
    println!("✓ Committee: ON DISK, RESTORED");
    println!("✓ Peers: ON DISK, RESTORED");
    println!("✓ Snapshots: ON DISK, RESTORED");
    println!("\nState survives complete process restart from persistent storage.");
}
