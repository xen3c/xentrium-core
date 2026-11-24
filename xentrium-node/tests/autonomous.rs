//! Autonomous network operation tests (Phase 4 Step 3).
//!
//! Spawns real Node instances over TLS on localhost and verifies the network
//! converges without any external admin driver:
//!   join -> receive validator/committee state -> pull snapshot -> converge.

use std::time::{Duration, Instant};
use xentrium_network::SnapshotSync;
use xentrium_node::{derive_committee, Node, NodeConfig};

fn test_dir(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "xnt-autonomous-{}-{}",
        std::process::id(),
        tag
    ));
    dir.to_string_lossy().to_string()
}

fn node_config(node_id: &str, port: u16, tag: &str, bootstrap: Vec<String>) -> NodeConfig {
    NodeConfig {
        node_id: node_id.to_string(),
        network_address: format!("127.0.0.1:{}", port),
        data_directory: test_dir(tag),
        bootstrap_peers: bootstrap,
        max_peers: 10,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 5,
        peer_timeout_secs: 60,
        allowed_node_ids: vec![],
        self_register_validator: true,
        announce_interval_secs: 5,
        committee_epoch_secs: 8,
        snapshot_epoch_secs: 8,
        committee_max_size: 51,
        snapshot_retention: 10,
        metrics_port: 0,
    }
}

/// Poll `check` until it returns true or the deadline expires.
async fn wait_for<F, Fut>(timeout_secs: u64, label: &str, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    while Instant::now() < deadline {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    eprintln!("TIMEOUT waiting for: {}", label);
    false
}

fn init_logs() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_autonomous_convergence_three_nodes() {
    init_logs();
    // node1 = seed, node2 + node3 bootstrap through it (star topology).
    // node3 learns node2's validator only via node1's relay — exercises
    // announcement gossip + envelope relay end to end.
    let seed_addr = "127.0.0.1:46101".to_string();

    let node1 = Node::new_with_storage(node_config("it-node-1", 46101, "n1", vec![]))
        .await
        .expect("node1 create");
    let node2 = Node::new_with_storage(node_config(
        "it-node-2",
        46102,
        "n2",
        vec![seed_addr.clone()],
    ))
    .await
    .expect("node2 create");
    let node3 = Node::new_with_storage(node_config(
        "it-node-3",
        46103,
        "n3",
        vec![seed_addr.clone()],
    ))
    .await
    .expect("node3 create");

    node1.start().await.expect("node1 start");
    node2.start().await.expect("node2 start");
    node3.start().await.expect("node3 start");

    // 1) Validator convergence: all three nodes see all three validators
    //    (node3's view of node2 arrives via relayed envelope through node1).
    let converged = wait_for(120, "validator set convergence (3 nodes)", || async {
        let v1 = node1.validator_manager().read().await.active_validator_count();
        let v2 = node2.validator_manager().read().await.active_validator_count();
        let v3 = node3.validator_manager().read().await.active_validator_count();
        v1 == 3 && v2 == 3 && v3 == 3
    })
    .await;
    assert!(converged, "all nodes should see 3 active validators");

    // 2) Committee formation: identical, non-empty committee on all nodes
    let committee_ok = wait_for(60, "committee formation", || async {
        let c1 = node1.validator_manager().read().await.get_committee_members();
        let c2 = node2.validator_manager().read().await.get_committee_members();
        let c3 = node3.validator_manager().read().await.get_committee_members();
        !c1.is_empty() && c1 == c2 && c2 == c3
    })
    .await;
    assert!(committee_ok, "committee should be formed and identical");

    // 3) Snapshot sync: leader produces per epoch, peers pull via
    //    SnapshotRequest -> SnapshotData -> install.
    let snap_ok = wait_for(90, "snapshot pull + install", || async {
        let s1 = node1.snapshot_sync().read().await.get_current_state_number();
        let s2 = node2.snapshot_sync().read().await.get_current_state_number();
        let s3 = node3.snapshot_sync().read().await.get_current_state_number();
        s1 > 0 && s1 == s2 && s2 == s3
    })
    .await;
    assert!(snap_ok, "all nodes should converge on the same snapshot state");

    // 4) Persistence: receivers persisted received state — restarting node3
    //    must reload validators + committee + snapshot from its own storage.
    node3.stop().await.expect("node3 stop");
    let node3b = Node::new_with_storage(node_config(
        "it-node-3",
        46103,
        "n3", // same data dir -> same identity + same DB
        vec![seed_addr.clone()],
    ))
    .await
    .expect("node3 recreate");
    node3b.start().await.expect("node3 restart");

    // load_persisted_state runs inside start(), before peers reconnect —
    // state must already be present immediately after start returns.
    {
        let vm_arc = node3b.validator_manager();
        let vm = vm_arc.read().await;
        assert_eq!(
            vm.active_validator_count(),
            3,
            "restarted node should reload all validators from storage"
        );
        assert!(
            !vm.get_committee_members().is_empty(),
            "restarted node should reload committee from storage"
        );
    }
    {
        let sync_arc = node3b.snapshot_sync();
        let sync = sync_arc.read().await;
        assert!(
            sync.get_current_state_number() > 0,
            "restarted node should reload snapshot state from storage"
        );
    }

    node1.stop().await.ok();
    node2.stop().await.ok();
    node3b.stop().await.ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn test_late_joiner_syncs_state() {
    init_logs();
    // Seed + one peer converge first; a late joiner must then pull the
    // full state via serve-on-connect + sync beacon.
    let seed_addr = "127.0.0.1:46201".to_string();

    let node1 = Node::new_with_storage(node_config("lj-node-1", 46201, "lj1", vec![]))
        .await
        .expect("node1 create");
    let node2 = Node::new_with_storage(node_config(
        "lj-node-2",
        46202,
        "lj2",
        vec![seed_addr.clone()],
    ))
    .await
    .expect("node2 create");

    node1.start().await.expect("node1 start");
    node2.start().await.expect("node2 start");

    let ready = wait_for(90, "initial 2-node convergence", || async {
        let v1 = node1.validator_manager().read().await.active_validator_count();
        let v2 = node2.validator_manager().read().await.active_validator_count();
        let s1 = node1.snapshot_sync().read().await.get_current_state_number();
        v1 == 2 && v2 == 2 && s1 > 0
    })
    .await;
    assert!(ready, "seed pair should converge before late joiner arrives");

    // Late joiner connects — must autonomously converge
    let node3 = Node::new_with_storage(node_config(
        "lj-node-3",
        46203,
        "lj3",
        vec![seed_addr.clone()],
    ))
    .await
    .expect("node3 create");
    node3.start().await.expect("node3 start");

    let joined = wait_for(120, "late joiner convergence", || async {
        let v3 = node3.validator_manager().read().await.active_validator_count();
        let c3 = node3.validator_manager().read().await.get_committee_members();
        let s3 = node3.snapshot_sync().read().await.get_current_state_number();
        v3 == 3 && !c3.is_empty() && s3 > 0
    })
    .await;
    assert!(joined, "late joiner should converge on validators+committee+snapshot");

    node1.stop().await.ok();
    node2.stop().await.ok();
    node3.stop().await.ok();
}

#[test]
fn test_derive_committee_deterministic() {
    let validators = vec![
        "xnt-ccc".to_string(),
        "xnt-aaa".to_string(),
        "xnt-bbb".to_string(),
    ];
    // Same input -> same output regardless of order
    let a = derive_committee(validators.clone(), 0, 51);
    let mut shuffled = validators.clone();
    shuffled.reverse();
    let b = derive_committee(shuffled, 0, 51);
    assert_eq!(a, b);
    assert_eq!(a.len(), 3);
    // Rotation: epoch shifts leadership deterministically
    let epoch1 = derive_committee(validators.clone(), 1, 51);
    assert_eq!(a.len(), epoch1.len());
    assert_ne!(a[0], epoch1[0], "rotation should change the leader");
    // Rotation is a permutation — same membership
    let mut sorted_a = a.clone();
    let mut sorted_e1 = epoch1.clone();
    sorted_a.sort();
    sorted_e1.sort();
    assert_eq!(sorted_a, sorted_e1);
    // Empty set -> empty committee
    assert!(derive_committee(vec![], 0, 51).is_empty());
    // Cap truncates deterministically
    let capped = derive_committee(validators.clone(), 0, 2);
    assert_eq!(capped.len(), 2);
    assert!(a.contains(&capped[0]) && a.contains(&capped[1]));
}
