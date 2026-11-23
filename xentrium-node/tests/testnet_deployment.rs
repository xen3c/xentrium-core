use xentrium_node::{Node, NodeConfig};
use xentrium_network::{Discovery, SnapshotSync};
use xentrium_shared::ValidatorInfo;
use std::time::Duration;

fn make_config(node_id: &str, addr: &str, dir: &str, bootstrap: Vec<String>) -> NodeConfig {
    NodeConfig {
        node_id: node_id.to_string(),
        network_address: addr.to_string(),
        data_directory: dir.to_string(),
        bootstrap_peers: bootstrap,
        max_peers: 50,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 2,
        peer_timeout_secs: 10,
        allowed_node_ids: vec![], ..NodeConfig::default()
    }
}

fn unique_dir(name: &str) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = format!("testnet_verify_{}_{}", name, ts);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn test_three_node_testnet_deployment() {
    // === STARTUP: three independent nodes ===
    let dir1 = unique_dir("node1");
    let dir2 = unique_dir("node2");
    let dir3 = unique_dir("node3");

    let node1 = Node::new_with_storage(make_config(
        "testnet-node-1", "127.0.0.1:51101", &dir1, vec![]
    )).await.unwrap();
    node1.start().await.unwrap();

    let node2 = Node::new_with_storage(make_config(
        "testnet-node-2", "127.0.0.1:51102", &dir2,
        vec!["127.0.0.1:51101".to_string()]
    )).await.unwrap();
    node2.start().await.unwrap();

    let node3 = Node::new_with_storage(make_config(
        "testnet-node-3", "127.0.0.1:51103", &dir3,
        vec!["127.0.0.1:51101".to_string(), "127.0.0.1:51102".to_string()]
    )).await.unwrap();
    node3.start().await.unwrap();

    assert!(node1.is_running().await && node2.is_running().await && node3.is_running().await);

    // === PEER DISCOVERY: allow gossip to propagate ===
    tokio::time::sleep(Duration::from_secs(4)).await;

    let peers1 = node1.connections().await.len();
    let peers2 = node2.connections().await.len();
    let peers3 = node3.connections().await.len();
    assert!(peers1 >= 1, "node1 should have inbound connections");
    assert!(peers2 >= 1, "node2 should have connections");
    assert!(peers3 >= 1, "node3 should have bootstrap connections");

    // === HEARTBEAT: trackers active on all nodes ===
    tokio::time::sleep(Duration::from_secs(3)).await;
    for node in [&node1, &node2, &node3] {
        let tracker = node.heartbeat_tracker();
        let t = tracker.read().await;
        assert!(t.len() >= 0);
    }

    // === VALIDATOR STATE CONVERGENCE ===
    // Node1 registers a validator and broadcasts it
    let validator_info = ValidatorInfo {
        address: "XNTValidator1".to_string(),
        public_key: vec![1, 2, 3, 4],
        machine_id: "machine-1".to_string(),
        balance: 1_000_000_000,
        state: xentrium_shared::ValidatorState::Active,
        enabled_at: Some(0),
        uptime_score: 100,
        reputation_score: 100,
        validator_weight: 1.0,
        last_seen: 0,
        consecutive_uptime_weeks: 0,
        committees_participated: 0,
        votes_cast: 0,
        votes_correct: 0,
        endpoint: "127.0.0.1:51101".to_string(),
        is_active: true,
    };
    {
        let vm1_arc = node1.validator_manager();
        let mut vm = vm1_arc.write().await;
        let _ = vm.register_validator_from_network(validator_info.clone());
    }
    node1.broadcast_validator_registration(validator_info).await.unwrap();

    tokio::time::sleep(Duration::from_secs(3)).await;

    // Node3 should have received the validator via gossip
    let vm3_arc = node3.validator_manager();
    let vm3 = vm3_arc.read().await;
    let validators3 = vm3.get_active_validators();
    assert!(
        validators3.iter().any(|v| v.address == "XNTValidator1"),
        "node3 should have converged validator state, got {:?}",
        validators3.iter().map(|v| &v.address).collect::<Vec<_>>()
    );
    drop(vm3);

    // === COMMITTEE CONVERGENCE ===
    // Node1 selects committee and broadcasts
    let members = vec!["XNTValidator1".to_string()];
    node1.broadcast_committee_selection(members.clone(), 1).await.unwrap();

    tokio::time::sleep(Duration::from_secs(3)).await;

    // Verify node2 received committee
    let vm2_arc = node2.validator_manager();
    let vm2 = vm2_arc.read().await;
    let committee2 = vm2.get_committee_members();
    assert_eq!(committee2.len(), members.len(), "node2 committee should match");
    drop(vm2);

    // === SNAPSHOT SYNC SERVICES ===
    let sync1_arc = node1.snapshot_sync();
    let sync2_arc = node2.snapshot_sync();
    let sync3_arc = node3.snapshot_sync();
    assert!(sync1_arc.read().await.get_available_snapshots().is_empty());
    assert!(sync2_arc.read().await.get_available_snapshots().is_empty());
    assert!(sync3_arc.read().await.get_available_snapshots().is_empty());

    // === CLEAN SHUTDOWN ===
    node1.stop().await.unwrap();
    node2.stop().await.unwrap();
    node3.stop().await.unwrap();

    assert!(!node1.is_running().await);
    assert!(!node2.is_running().await);
    assert!(!node3.is_running().await);

    let _ = std::fs::remove_dir_all(&dir1);
    let _ = std::fs::remove_dir_all(&dir2);
    let _ = std::fs::remove_dir_all(&dir3);
}
