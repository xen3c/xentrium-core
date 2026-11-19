use xentrium_network::{BasicSnapshotSync, PeerID, SnapshotSync};
use xentrium_shared::Snapshot;
use std::collections::HashMap;

fn create_test_snapshot(state_number: i64) -> Snapshot {
    let state_hash = format!("state_hash_{}", state_number);
    let timestamp = state_number * 1000;
    let snapshot_hash = format!("{}:{}:{}", state_number, state_hash, timestamp);
    
    Snapshot {
        state_number,
        state_hash,
        snapshot_hash,
        timestamp,
        accounts: HashMap::new(),
        active_validators: vec![],
        pending_validators: vec![],
        reward_pool_balance: 0,
        last_reward_distribution: 0,
        active_proposals: vec![],
        proposal_votes: HashMap::new(),
        committee_signature: None,
    }
}

#[tokio::test]
async fn test_snapshot_synchronization_flow() {
    // Node A owns snapshot
    let mut sync_a = BasicSnapshotSync::new(100);
    let snapshot = create_test_snapshot(200);
    sync_a.serve_snapshot(snapshot.clone()).unwrap();
    
    // Node B discovers snapshot
    let mut sync_b = BasicSnapshotSync::new(100);
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    // Node B requests snapshot
    sync_b.request_snapshot(peer_a.clone(), 200).unwrap();
    
    // Node A sends snapshot (simulated by direct receive)
    sync_b.receive_snapshot(snapshot).unwrap();
    
    // Verify synchronization success
    assert_eq!(sync_b.get_current_state_number(), 200);
}

#[tokio::test]
async fn test_corrupted_snapshot_rejection() {
    let mut sync = BasicSnapshotSync::new(100);
    
    // Create a snapshot with invalid hash
    let mut snapshot = create_test_snapshot(200);
    snapshot.snapshot_hash = "corrupted_hash".to_string();
    
    // Attempt to receive corrupted snapshot
    let result = sync.receive_snapshot(snapshot);
    assert!(result.is_err());
    
    // State number should not be updated
    assert_eq!(sync.get_current_state_number(), 100);
}

#[tokio::test]
async fn test_missing_snapshot_handling() {
    let mut sync_a = BasicSnapshotSync::new(100);
    let mut sync_b = BasicSnapshotSync::new(100);
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    // Node B requests snapshot that doesn't exist on A
    let result = sync_b.request_snapshot(peer_a, 200);
    assert!(result.is_ok()); // Request succeeds, but would fail at transport layer
    
    // Node A doesn't have the snapshot
    let available = sync_a.get_available_snapshots();
    assert_eq!(available.len(), 0);
}

#[tokio::test]
async fn test_newer_snapshot_selection() {
    let mut sync = BasicSnapshotSync::new(0);
    
    // Add multiple snapshots
    sync.serve_snapshot(create_test_snapshot(50)).unwrap();
    sync.serve_snapshot(create_test_snapshot(100)).unwrap();
    sync.serve_snapshot(create_test_snapshot(75)).unwrap();
    
    let available = sync.get_available_snapshots();
    
    // Should select the newest snapshot
    assert_eq!(available.len(), 3);
    assert_eq!(available[0].state_number, 100);
}

#[tokio::test]
async fn test_duplicate_request_handling() {
    let mut sync = BasicSnapshotSync::new(100);
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    // First request
    sync.request_snapshot(peer_a.clone(), 200).unwrap();
    
    // Duplicate request should fail
    let result = sync.request_snapshot(peer_a, 200);
    assert!(result.is_err());
}

#[tokio::test]
async fn test_snapshot_metadata_exchange() {
    let mut sync_a = BasicSnapshotSync::new(100);
    let mut sync_b = BasicSnapshotSync::new(100);
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    // Node A advertises snapshot
    let snapshot = create_test_snapshot(200);
    sync_a.serve_snapshot(snapshot.clone()).unwrap();
    
    let available = sync_a.get_available_snapshots();
    assert_eq!(available.len(), 1);
    
    // Node B can see the advertised snapshot
    let metadata = available[0].clone();
    sync_b.advertise_snapshot(metadata).unwrap();
    
    let available_b = sync_b.get_available_snapshots();
    assert_eq!(available_b.len(), 1);
    assert_eq!(available_b[0].state_number, 200);
}
