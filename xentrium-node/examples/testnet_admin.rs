//! Live testnet admin driver (Phase-4 Step-2 observation tooling).
//!
//! Joins the running testnet as a regular node and exercises the existing
//! public Node API — no protocol or feature changes:
//!   * register a validator   (broadcast + persist)
//!   * form a committee       (broadcast + persist)
//!   * publish a snapshot     (offer + data broadcast + persist)
//!   * `check` action re-loads persisted state after a restart
//!
//! Broadcasts only reach directly-connected peers, so pass every node you
//! want to observe as a bootstrap peer (seed endpoint, LAN endpoints, …).
//!
//! Usage:
//!   cargo run --release --example testnet_admin -- \
//!       --data-dir ./data/admin \
//!       --bootstrap "178.18.248.9:443,127.0.0.1:30304" \
//!       --action all
//!   cargo run --release --example testnet_admin -- \
//!       --data-dir ./data/admin --action check

use clap::Parser;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use xentrium_network::SnapshotSync;
use xentrium_node::{Node, NodeConfig};
use xentrium_shared::{Snapshot, ValidatorInfo, ValidatorState};

#[derive(Parser, Debug)]
#[command(name = "testnet_admin", about = "Live testnet activation driver")]
struct Args {
    /// Data directory for the driver node's identity/TLS/DB
    #[arg(long, default_value = "./data/admin")]
    data_dir: String,

    /// Comma-separated bootstrap peers (each receives the broadcasts)
    #[arg(long, default_value = "")]
    bootstrap: String,

    /// Action: all | register | committee | snapshot | check
    #[arg(long, default_value = "all")]
    action: String,

    /// Validator address to register
    #[arg(long, default_value = "test_validator")]
    validator_address: String,

    /// Snapshot/committee state number
    #[arg(long, default_value_t = 1)]
    state_number: i64,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let bootstrap_peers: Vec<String> = args
        .bootstrap
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let config = NodeConfig {
        node_id: "testnet-admin".to_string(),
        network_address: "127.0.0.1:30700".to_string(),
        data_directory: args.data_dir.clone(),
        bootstrap_peers,
        max_peers: 50,
        log_level: "info".to_string(),
        heartbeat_interval_secs: 30,
        peer_timeout_secs: 120,
        allowed_node_ids: vec![],
    };

    let node = Node::new_with_storage(config).await?;
    node.start().await?;

    // Wait for at least one connection + handshake to complete
    let deadline = SystemTime::now() + Duration::from_secs(30);
    loop {
        let conns = node.connections().await;
        if !conns.is_empty() || args.bootstrap.is_empty() {
            if !conns.is_empty() {
                println!("connected to: {:?}", conns);
            }
            break;
        }
        if SystemTime::now() > deadline {
            eprintln!("WARNING: no bootstrap connections after 30s — continuing anyway");
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    // Give handshake verification a moment to land on both sides
    tokio::time::sleep(Duration::from_secs(2)).await;

    let do_all = args.action == "all";

    if do_all || args.action == "register" {
        let validator = ValidatorInfo {
            address: args.validator_address.clone(),
            public_key: node.identity().public_key_bytes(),
            machine_id: "testnet-node-2".to_string(),
            balance: 100_000_000,
            state: ValidatorState::Active,
            enabled_at: Some(now_secs()),
            uptime_score: 86400,
            reputation_score: 100,
            validator_weight: 1.0,
            last_seen: now_secs(),
            consecutive_uptime_weeks: 0,
            committees_participated: 0,
            votes_cast: 0,
            votes_correct: 0,
            endpoint: "testnet-node-2".to_string(),
            is_active: true,
        };
        node.broadcast_validator_registration(validator.clone()).await?;
        match node.persist_validator(&validator).await {
            Ok(()) => println!("ACTION register: broadcast + persisted validator '{}'", validator.address),
            Err(e) => println!("ACTION register: broadcast sent; persist skipped ({})", e),
        }
    }

    if do_all || args.action == "committee" {
        let members = vec![args.validator_address.clone()];
        node.broadcast_committee_selection(members.clone(), args.state_number).await?;
        match node.persist_committee(&members, args.state_number).await {
            Ok(()) => println!("ACTION committee: broadcast + persisted committee {:?} (state {})", members, args.state_number),
            Err(e) => println!("ACTION committee: broadcast sent; persist skipped ({})", e),
        }
    }

    if do_all || args.action == "snapshot" {
        let ts = now_secs();
        let state_hash = format!("testnet-state-{}", args.state_number);
        let snapshot = Snapshot {
            state_number: args.state_number,
            snapshot_hash: format!("{}:{}:{}", args.state_number, state_hash, ts),
            state_hash,
            timestamp: ts,
            accounts: HashMap::new(),
            active_validators: vec![args.validator_address.clone()],
            pending_validators: vec![],
            reward_pool_balance: 0,
            last_reward_distribution: 0,
            active_proposals: vec![],
            proposal_votes: HashMap::new(),
            committee_signature: None,
        };
        match node.persist_snapshot(&snapshot).await {
            Ok(()) => println!("ACTION snapshot: persisted state {}", args.state_number),
            Err(e) => println!("ACTION snapshot: persist skipped ({})", e),
        }
        node.broadcast_snapshot_offer(&snapshot).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        node.broadcast_snapshot_data(&snapshot).await?;
        println!("ACTION snapshot: persisted + offered + broadcast state {}", args.state_number);
    }

    // Give messages time to arrive on peers
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Local verification dump (useful for both `all` and `check`)
    {
        let vm = node.validator_manager();
        let vm = vm.read().await;
        let active = vm.get_active_validators();
        let pending = vm.get_pending_validators();
        let committee = vm.get_committee_members();
        println!("STATE validators: {} active, {} pending -> {:?} / {:?}",
            active.len(), pending.len(),
            active.iter().map(|v| v.address.clone()).collect::<Vec<_>>(),
            pending.iter().map(|v| v.address.clone()).collect::<Vec<_>>());
        println!("STATE committee: {} members -> {:?}", committee.len(), committee);
    }
    {
        let sync = node.snapshot_sync();
        let sync = sync.read().await;
        let snaps = sync.get_available_snapshots();
        println!("STATE snapshots: {} available, current_state={}",
            snaps.len(), sync.get_current_state_number());
        for s in &snaps {
            println!("  - state {} hash {}", s.state_number, s.snapshot_hash);
        }
    }

    node.stop().await?;
    println!("done.");
    Ok(())
}
