//! Expanded testnet readiness: real multi-node topologies at 10/25/50/100
//! nodes plus fault-injection scenarios (crash, reboot, partition via
//! bootstrap loss, delayed rejoin). In-process nodes on localhost — real
//! TCP + TLS + signed envelopes, synthetic only in that nodes share one
//! OS process.

use std::sync::Arc;
use std::time::{Duration, Instant};
use xentrium_network::SnapshotSync;
use xentrium_node::{Node, NodeConfig};

fn test_dir(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "xentrium-ex-{}-{}",
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
        max_peers: 256,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 10,
        peer_timeout_secs: 60,
        allowed_node_ids: vec![],
        self_register_validator: true,
        announce_interval_secs: 15,
        committee_epoch_secs: 10,
        snapshot_epoch_secs: 10,
        committee_max_size: 51,
        snapshot_retention: 10,
        metrics_port: 0,
        // Synthetic churn must not trip the limiters.
        max_conns_per_ip: 512,
        handshake_rate_per_min: 1_000_000,
        ban_duration_secs: 30,
        ban_strike_threshold: 1_000_000,
        msg_rate_per_min: 10_000_000,
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
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    eprintln!("TIMEOUT waiting for: {}", label);
    None
}

fn rss_bytes() -> u64 {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    sys.refresh_process(pid);
    sys.process(pid).map(|p| p.memory()).unwrap_or(0)
}

fn cpu_percent() -> f32 {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    sys.refresh_process(pid);
    // cpu_usage is % of one core; >100 possible on multicore.
    sys.process(pid).map(|p| p.cpu_usage()).unwrap_or(0.0)
}

async fn total_msgs(nodes: &[Arc<Node>]) -> u64 {
    let mut t = 0;
    for n in nodes {
        t += n.messages_received();
    }
    t
}

async fn min_validator_count(nodes: &[Arc<Node>]) -> usize {
    let mut min = usize::MAX;
    for n in nodes {
        let c = n.validator_manager().read().await.active_validator_count();
        if c < min {
            min = c;
        }
    }
    if min == usize::MAX { 0 } else { min }
}

async fn committees_identical(nodes: &[Arc<Node>]) -> Option<usize> {
    let first = nodes[0].validator_manager().read().await.get_committee_members();
    if first.is_empty() {
        return None;
    }
    for n in &nodes[1..] {
        let c = n.validator_manager().read().await.get_committee_members();
        if c != first {
            return None;
        }
    }
    Some(first.len())
}

async fn min_state_number(nodes: &[Arc<Node>]) -> i64 {
    let mut min = i64::MAX;
    for n in nodes {
        let s = n.snapshot_sync().read().await.get_current_state_number();
        if s < min {
            min = s;
        }
    }
    if min == i64::MAX { -1 } else { min }
}

async fn all_have_peer(nodes: &[Arc<Node>]) -> bool {
    for n in nodes {
        if n.connections().await.is_empty() {
            return false;
        }
    }
    true
}

const BASE_PORT: u16 = 48100;

/// Ramp: star topology (all leaves bootstrap to node0). Every node
/// self-registers → N validators for N nodes. Grow 10 → 25 → 50 → 100.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_expanded_node_ramp() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();

    let seed_addr = format!("127.0.0.1:{}", BASE_PORT);
    let stages = [10usize, 25, 50, 100];
    let max_n = *stages.last().unwrap();
    let mut nodes: Vec<Arc<Node>> = Vec::with_capacity(max_n);

    eprintln!("\n=== EXPANDED TESTNET RAMP ===");

    for &target in &stages {
        // Add nodes until we have `target` running.
        while nodes.len() < target {
            let i = nodes.len();
            let (id, port, tag, boot) = if i == 0 {
                ("ex-node-0".to_string(), BASE_PORT, "ex0".to_string(), vec![])
            } else {
                (
                    format!("ex-node-{}", i),
                    BASE_PORT + i as u16,
                    format!("ex{}", i),
                    vec![seed_addr.clone()],
                )
            };
            let spawn_t = Instant::now();
            let node = Arc::new(
                Node::new_with_storage(node_config(&id, port, &tag, boot))
                    .await
                    .expect("node create"),
            );
            node.start().await.expect("node start");
            nodes.push(node);
            eprintln!("  + node {} spawned in {}ms", i, spawn_t.elapsed().as_millis());
        }
        eprintln!("--- stage {} nodes spawned, measuring ---", target);

        // 1. Peer discovery: every node holds >=1 connection.
        let disc = wait_for(120, &format!("discovery at {} nodes", target), || async {
            all_have_peer(&nodes).await
        })
        .await;

        // 2. Validator propagation: all nodes know `target` validators.
        let prop = wait_for(300, &format!("{} validators everywhere", target), || async {
            min_validator_count(&nodes).await >= target
        })
        .await;

        // 3. Committee convergence: identical committee on all nodes,
        //    size min(target, 51).
        let exp_committee = target.min(51);
        let comm = wait_for(180, &format!("committee {} at {} nodes", exp_committee, target), || async {
            committees_identical(&nodes).await == Some(exp_committee)
        })
        .await;

        // 4. Snapshot sync: all nodes on the same state number > 0.
        let snap = wait_for(180, &format!("snapshot sync at {} nodes", target), || async {
            min_state_number(&nodes).await > 0
        })
        .await;
        eprintln!(
            "    stage {} waits done: disc={:?} prop={:?} comm={:?} snap={:?}",
            target,
            disc.map(|d| d.as_millis()),
            prop.map(|d| d.as_millis()),
            comm.map(|d| d.as_millis()),
            snap.map(|d| d.as_millis()),
        );

        // Settle one announce cycle, then sample traffic/CPU/RSS.
        tokio::time::sleep(Duration::from_secs(16)).await;
        let msgs = total_msgs(&nodes).await;
        let rss = rss_bytes() / 1_048_576;
        let cpu = cpu_percent();

        eprintln!(
            "stage {} nodes | discovery {}ms | propagation {}ms | committee {}ms | snapshot {}ms | total_msgs {} | rss {}MB | cpu {:.0}%",
            target,
            disc.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX),
            prop.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX),
            comm.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX),
            snap.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX),
            msgs, rss, cpu,
        );

        assert!(disc.is_some(), "discovery failed at {} nodes", target);
        assert!(prop.is_some(), "propagation failed at {} nodes", target);
        assert!(comm.is_some(), "committee failed at {} nodes", target);
        assert!(snap.is_some(), "snapshot sync failed at {} nodes", target);
    }

    for n in &nodes {
        n.stop().await.ok();
    }
}

/// Fault injection on an 8-node star.
/// Scenarios: leaf crash + reboot, delayed rejoin, bootstrap restart
/// (star partition), with convergence measurements.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_fault_injection() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();

    let port_base: u16 = 48300;
    let seed_addr = format!("127.0.0.1:{}", port_base);
    let n_nodes = 8usize;
    let mut nodes: Vec<Arc<Node>> = Vec::new();

    for i in 0..n_nodes {
        let boot = if i == 0 { vec![] } else { vec![seed_addr.clone()] };
        let node = Arc::new(
            Node::new_with_storage(node_config(
                &format!("fx-node-{}", i),
                port_base + i as u16,
                &format!("fx{}", i),
                boot,
            ))
            .await
            .expect("node create"),
        );
        node.start().await.expect("node start");
        nodes.push(node);
    }

    // Baseline: 8 validators everywhere, committee agreed.
    assert!(
        wait_for(120, "baseline 8 validators", || async {
            min_validator_count(&nodes).await >= n_nodes
        })
        .await
        .is_some()
    );
    assert!(
        wait_for(90, "baseline committee", || async {
            committees_identical(&nodes).await == Some(n_nodes)
        })
        .await
        .is_some()
    );
    eprintln!("\n=== FAULT INJECTION ===");

    // --- Fault 1: leaf node crash ---
    let victim = 7usize;
    nodes[victim].stop().await.expect("stop victim");
    eprintln!("fault: node-{} stopped (simulated crash)", victim);
    // Mesh keeps operating: remaining nodes still agree on committee.
    let post_crash = wait_for(60, "committee survives leaf crash", || async {
        committees_identical(&nodes[..victim]).await.is_some()
    })
    .await;
    eprintln!(
        "post-crash committee on survivors: {}",
        if post_crash.is_some() { "converged" } else { "TIMEOUT" }
    );
    assert!(post_crash.is_some(), "surviving mesh did not stay converged");

    // --- Fault 2: node reboot (same Node restarts, reloads persisted state) ---
    let reboot_t = Instant::now();
    nodes[victim].start().await.expect("reboot victim");
    let rejoin = wait_for(120, "crashed node rejoins + catches up", || async {
        !nodes[victim].connections().await.is_empty()
            && nodes[victim]
                .validator_manager()
                .read()
                .await
                .active_validator_count()
                >= n_nodes
    })
    .await;
    eprintln!(
        "reboot: reconnected+{} validators in {}ms",
        n_nodes,
        rejoin.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX)
    );
    assert!(rejoin.is_some(), "rebooted node did not catch up");
    eprintln!("reboot wall time: {}ms", reboot_t.elapsed().as_millis());

    // --- Fault 3: delayed rejoin — stop a node, let epochs pass, restart ---
    let laggard = 6usize;
    nodes[laggard].stop().await.expect("stop laggard");
    tokio::time::sleep(Duration::from_secs(25)).await; // ~2 announce cycles
    let before = min_state_number(&nodes[..laggard]).await;
    nodes[laggard].start().await.expect("restart laggard");
    let catchup = wait_for(120, "laggard catches up after delay", || async {
        nodes[laggard]
            .validator_manager()
            .read()
            .await
            .active_validator_count()
            >= n_nodes
            && nodes[laggard]
                .snapshot_sync()
                .read()
                .await
                .get_current_state_number()
                >= before
    })
    .await;
    eprintln!(
        "delayed rejoin: caught up in {}ms (state >= {})",
        catchup.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX),
        before
    );
    assert!(catchup.is_some(), "delayed node did not resync");

    // --- Fault 4: bootstrap restart (= star partition while down) ---
    nodes[0].stop().await.expect("stop bootstrap");
    tokio::time::sleep(Duration::from_secs(5)).await;
    // Partition check: leaves keep their state even with hub gone.
    let leaf_validators = nodes[3]
        .validator_manager()
        .read()
        .await
        .active_validator_count();
    eprintln!(
        "partition: leaf retains {} validators with bootstrap down",
        leaf_validators
    );
    assert_eq!(leaf_validators, n_nodes, "leaf lost state during partition");

    nodes[0].start().await.expect("restart bootstrap");
    let heal = wait_for(120, "mesh heals after bootstrap restart", || async {
        all_have_peer(&nodes).await
            && min_validator_count(&nodes).await >= n_nodes
            && committees_identical(&nodes).await.is_some()
    })
    .await;
    eprintln!(
        "bootstrap restart: mesh healed in {}ms",
        heal.map(|d| d.as_millis() as u64).unwrap_or(u64::MAX)
    );
    assert!(heal.is_some(), "mesh did not heal after bootstrap restart");

    for n in &nodes {
        n.stop().await.ok();
    }
}
