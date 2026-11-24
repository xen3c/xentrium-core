//! Scale audit (Phase 4 Step 5).
//!
//! Ramps the validator set on a real 3-node TLS network (star topology,
//! same as the live testnet) to 10 -> 25 -> 50 validators. Synthetic
//! validators are injected as real signed ValidatorRegistration
//! envelopes broadcast by the seed, so they traverse the same
//! receive/verify/relay path as genuine registrations.
//!
//! Measures per stage: propagation latency, committee formation,
//! snapshot production, and on-disk database growth.

use std::time::{Duration, Instant};
use xentrium_network::SnapshotSync;
use xentrium_shared::{ValidatorInfo, ValidatorState};
use xentrium_node::{Node, NodeConfig};

fn test_dir(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "xnt-scale-{}-{}",
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
        max_peers: 64,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 5,
        peer_timeout_secs: 60,
        allowed_node_ids: vec![],
        self_register_validator: true,
        announce_interval_secs: 5,
        committee_epoch_secs: 8,
        snapshot_epoch_secs: 8,
    }
}

fn synthetic_validator(i: usize) -> ValidatorInfo {
    ValidatorInfo {
        address: format!("xnt-scale-{:04}", i),
        public_key: vec![i as u8; 32],
        machine_id: format!("scale-machine-{}", i),
        balance: 1_000_000,
        state: ValidatorState::Active,
        enabled_at: Some(0),
        uptime_score: 100,
        reputation_score: 0,
        validator_weight: 1.0,
        last_seen: 0,
        consecutive_uptime_weeks: 0,
        committees_participated: 0,
        votes_cast: 0,
        votes_correct: 0,
        endpoint: format!("scale-{}.internal:0", i),
        is_active: true,
    }
}

async fn wait_for<F, Fut>(timeout_secs: u64, label: &str, mut check: F) -> Option<Duration>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = Instant::now();
    let deadline = start + Duration::from_secs(timeout_secs);
    while Instant::now() < deadline {
        if check().await {
            return Some(start.elapsed());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!("TIMEOUT waiting for: {}", label);
    None
}

fn db_size(tag: &str) -> u64 {
    std::fs::metadata(format!("{}/xentrium.db", test_dir(tag)))
        .map(|m| m.len())
        .unwrap_or(0)
}

fn init_logs() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
}

/// Ramp test: 3-node star network, inject validators to reach
/// 10 -> 25 -> 50 total, measure each stage.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn test_scale_ramp_10_25_50() {
    init_logs();
    let seed_addr = "127.0.0.1:47101".to_string();

    let node1 = Node::new_with_storage(node_config("sc-node-1", 47101, "s1", vec![]))
        .await
        .expect("node1 create");
    let node2 = Node::new_with_storage(node_config(
        "sc-node-2",
        47102,
        "s2",
        vec![seed_addr.clone()],
    ))
    .await
    .expect("node2 create");
    let node3 = Node::new_with_storage(node_config(
        "sc-node-3",
        47103,
        "s3",
        vec![seed_addr.clone()],
    ))
    .await
    .expect("node3 create");

    node1.start().await.expect("node1 start");
    node2.start().await.expect("node2 start");
    node3.start().await.expect("node3 start");

    // Wait for the 3 real self-registrations to converge first.
    let base = wait_for(60, "baseline 3-validator convergence", || async {
        node1.validator_manager().read().await.active_validator_count() == 3
            && node2.validator_manager().read().await.active_validator_count() == 3
            && node3.validator_manager().read().await.active_validator_count() == 3
    })
    .await;
    assert!(base.is_some(), "baseline convergence failed");

    let stages = [10usize, 25, 50];
    let mut injected = 0usize; // synthetic validators injected so far

    eprintln!("\n=== SCALE AUDIT ===");
    for &target in &stages {
        // Inject synthetic registrations until node1 knows `target`
        // validators (3 real + synthetic).
        let need = target.saturating_sub(3);
        while injected < need {
            injected += 1;
            node1
                .broadcast_validator_registration(synthetic_validator(injected))
                .await
                .expect("broadcast synthetic validator");
            // Register locally too (broadcast doesn't loop back).
            node1
                .validator_manager()
                .write()
                .await
                .register_validator_from_network(synthetic_validator(injected))
                .ok();
            if let Some(repo) = node1.validator_repository() {
                repo.register(&synthetic_validator(injected)).await.ok();
            }
        }

        // Propagation: all three nodes reach `target` active validators.
        let prop = wait_for(120, &format!("{} validators on all nodes", target), || async {
            node1.validator_manager().read().await.active_validator_count() == target
                && node2.validator_manager().read().await.active_validator_count() == target
                && node3.validator_manager().read().await.active_validator_count() == target
        })
        .await;
        let prop_ms = prop.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX);

        // Committee: all nodes agree on an identical committee
        // (committee = full rotated validator set).
        let comm = wait_for(60, &format!("committee of {}", target), || async {
            let c1 = node1.validator_manager().read().await.get_committee_members();
            let c2 = node2.validator_manager().read().await.get_committee_members();
            let c3 = node3.validator_manager().read().await.get_committee_members();
            c1.len() == target && c1 == c2 && c2 == c3
        })
        .await;
        let comm_ms = comm.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX);

        // Snapshot: leader produces and peers pull — state number > 0 on all.
        let snap = wait_for(90, "snapshot converged", || async {
            let s1 = node1.snapshot_sync().read().await.get_current_state_number();
            let s2 = node2.snapshot_sync().read().await.get_current_state_number();
            let s3 = node3.snapshot_sync().read().await.get_current_state_number();
            s1 > 0 && s1 == s2 && s2 == s3
        })
        .await;
        let snap_ms = snap.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX);

        eprintln!(
            "stage {} validators | propagation {}ms | committee {}ms | snapshot {}ms | db: seed={}B n2={}B n3={}B",
            target, prop_ms, comm_ms, snap_ms,
            db_size("s1"), db_size("s2"), db_size("s3"),
        );

        assert!(prop.is_some(), "{} validators failed to propagate", target);
        assert!(comm.is_some(), "committee of {} failed to converge", target);
        assert!(snap.is_some(), "snapshot did not converge at {}", target);
    }

    // Memory leak / growth sanity: validator map sizes should be stable
    // (50 entries each — no unbounded per-message structures).
    {
        let vm_arc = node1.validator_manager();
        let vm = vm_arc.read().await;
        assert_eq!(vm.active_validator_count(), 50);
    }
    let ann = node1.known_announcement_count().await;
    eprintln!("node1 known announcements: {}", ann);

    node1.stop().await.ok();
    node2.stop().await.ok();
    node3.stop().await.ok();
}
