use clap::Parser;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};
use xentrium_node::{Node, NodeConfig};
use xentrium_node::config::NodeConfig as TomlConfig;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Xentrium Protocol Node
#[derive(Parser, Debug)]
#[command(name = "xentrium")]
#[command(about = "Xentrium blockchain node", long_about = None)]
struct Args {
    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,

    /// Node ID (overrides config)
    #[arg(short, long)]
    node_id: Option<String>,

    /// Network address (overrides config)
    #[arg(short, long)]
    address: Option<String>,

    /// Network port (overrides config)
    #[arg(short, long)]
    port: Option<u16>,

    /// Data directory (overrides config)
    #[arg(short, long)]
    data_dir: Option<String>,

    /// Bootstrap peers (overrides config)
    #[arg(short, long)]
    bootstrap: Option<Vec<String>>,

    /// Log level (overrides config)
    #[arg(short, long)]
    log_level: Option<String>,

    /// Run pre-flight deployment checks and exit (no node start)
    #[arg(long)]
    doctor: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Load configuration
    let mut toml_config = TomlConfig::load_or_default(args.config.as_deref());

    // Apply CLI overrides
    if let Some(node_id) = args.node_id {
        toml_config.node.node_id = node_id;
    }
    if let Some(address) = args.address {
        toml_config.node.address = address;
    }
    if let Some(port) = args.port {
        toml_config.node.port = port;
    }
    if let Some(data_dir) = args.data_dir {
        toml_config.node.data_dir = data_dir;
    }
    if let Some(bootstrap) = args.bootstrap {
        toml_config.network.bootstrap_peers = bootstrap;
    }
    if let Some(log_level) = args.log_level {
        toml_config.logging.level = log_level;
    }

    if args.doctor {
        return run_doctor(&toml_config);
    }

    // Initialize logging
    init_logging(&toml_config.logging);

    info!("Starting Xentrium node: {}", toml_config.node.node_id);
    info!("Network address: {}", toml_config.network_address());
    info!("Data directory: {}", toml_config.data_directory());

    // Create data directory if it doesn't exist
    std::fs::create_dir_all(&toml_config.data_directory())?;

    // Create node configuration
    let node_config = NodeConfig {
        node_id: toml_config.node.node_id.clone(),
        network_address: toml_config.network_address(),
        data_directory: toml_config.data_directory(),
        bootstrap_peers: toml_config.network.bootstrap_peers.clone(),
        max_peers: toml_config.network.max_peers,
        log_level: toml_config.logging.level.clone(),
        heartbeat_interval_secs: toml_config.network.heartbeat_interval_secs,
        peer_timeout_secs: toml_config.network.connection_timeout_secs,
        allowed_node_ids: toml_config.network.allowed_node_ids.clone(),
        self_register_validator: toml_config.consensus.self_register_validator,
        announce_interval_secs: toml_config.consensus.announce_interval_secs,
        committee_epoch_secs: toml_config.consensus.committee_epoch_secs,
        snapshot_epoch_secs: toml_config.consensus.snapshot_epoch_secs,
    };

    // Create node
    let node = Node::new_with_storage(node_config).await?;
    node.start().await?;

    // Setup signal handling
    let shutdown_signal = Arc::new(AtomicBool::new(false));
    let shutdown_signal_clone = shutdown_signal.clone();

    // Handle CTRL+C on all platforms
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        
        tokio::spawn(async move {
            tokio::select! {
                _ = sigint.recv() => {
                    info!("Received SIGINT, initiating graceful shutdown...");
                    shutdown_signal_clone.store(true, Ordering::SeqCst);
                }
                _ = sigterm.recv() => {
                    info!("Received SIGTERM, initiating graceful shutdown...");
                    shutdown_signal_clone.store(true, Ordering::SeqCst);
                }
            }
        });
    }

    #[cfg(windows)]
    {
        use tokio::signal::windows::ctrl_c;
        let mut ctrl_c = ctrl_c()?;
        
        tokio::spawn(async move {
            if ctrl_c.recv().await.is_some() {
                info!("Received CTRL+C, initiating graceful shutdown...");
                shutdown_signal_clone.store(true, Ordering::SeqCst);
            }
        });
    }

    // Operational loop
    info!("Node operational");
    info!("Press CTRL+C to shutdown");

    while !shutdown_signal.load(Ordering::SeqCst) {
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Graceful shutdown
    info!("Shutting down node...");
    node.stop().await?;
    info!("Node shutdown complete");

    Ok(())
}

/// Pre-flight deployment checks: config, identity, TLS credentials, port, peers.
/// Safe to run before launch; provisions identity/TLS files exactly as node start would.
fn run_doctor(config: &TomlConfig) -> Result<(), Box<dyn std::error::Error>> {
    let mut failures = 0;
    let mut check = |ok: bool, label: &str, detail: String| {
        if ok {
            println!("  [PASS] {} — {}", label, detail);
        } else {
            println!("  [FAIL] {} — {}", label, detail);
            failures += 1;
        }
    };

    println!("Xentrium node doctor — pre-flight checks\n");

    // 1. Network address parses and port is bindable
    let net_addr = config.network_address();
    let addr_ok = net_addr.parse::<std::net::SocketAddr>().is_ok();
    check(addr_ok, "network address", format!("{}", net_addr));

    if addr_ok {
        let addr: std::net::SocketAddr = net_addr.parse().unwrap();
        match std::net::TcpListener::bind(addr) {
            Ok(l) => {
                check(true, "port availability", format!("{} is free", addr));
                drop(l);
            }
            Err(e) => check(false, "port availability", format!("{} — {}", addr, e)),
        }
    }

    // 2. Data directory writable
    let data_dir = config.data_directory();
    let dir_ok = std::fs::create_dir_all(&data_dir).is_ok();
    check(dir_ok, "data directory", data_dir.clone());
    if dir_ok {
        let probe = std::path::Path::new(&data_dir).join(".doctor_probe");
        let writable = std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        check(writable, "data directory writable", data_dir.clone());
    }

    // 3. Node identity (load_or_create provisions it, printing the node_id)
    let id_path = std::path::Path::new(&data_dir).join("node.identity");
    match xentrium_network::NodeIdentity::load_or_create(&id_path) {
        Ok(identity) => check(true, "node identity", format!("node_id={} ({})", identity.node_id, id_path.display())),
        Err(e) => check(false, "node identity", format!("{}", e)),
    }

    // 4. TLS credentials (load_or_generate provisions + validates cert/key pair)
    if dir_ok {
        let tls_dir = xentrium_network::tls_dir(&data_dir);
        // Reuse identity node_id if available
        if let Ok(identity) = xentrium_network::NodeIdentity::load_or_create(&id_path) {
            match xentrium_network::TlsCredentials::load_or_generate(&tls_dir, &identity.node_id) {
                Ok(_) => check(true, "TLS credentials", format!("cert CN bound to {} in {}", identity.node_id, tls_dir.display())),
                Err(e) => check(false, "TLS credentials", format!("{}", e)),
            }
        }
    }

    // 5. Bootstrap peers parse
    let mut peer_ok = true;
    let mut peer_details = Vec::new();
    for p in &config.network.bootstrap_peers {
        match p.parse::<std::net::SocketAddr>() {
            Ok(a) => peer_details.push(format!("{}", a)),
            Err(_) => { peer_ok = false; peer_details.push(format!("INVALID:{}", p)); }
        }
    }
    check(peer_ok, "bootstrap peers", if peer_details.is_empty() { "none (seed node)".to_string() } else { peer_details.join(", ") });

    // 6. Allowlist
    check(true, "peer allowlist", if config.network.allowed_node_ids.is_empty() {
        "empty — open testnet mode".to_string()
    } else {
        format!("{} allowed node_id(s)", config.network.allowed_node_ids.len())
    });

    // 7. Logging level sane
    let lvl_ok = ["trace", "debug", "info", "warn", "error"].contains(&config.logging.level.as_str());
    check(lvl_ok, "log level", config.logging.level.clone());

    println!();
    if failures == 0 {
        println!("DOCTOR: ALL CHECKS PASSED — node is ready to start");
        Ok(())
    } else {
        println!("DOCTOR: {} CHECK(S) FAILED — fix before starting", failures);
        Err("doctor checks failed".into())
    }
}

fn init_logging(config: &xentrium_node::config::LoggingSection) {
    let level = config.level.parse::<tracing::Level>().unwrap_or(tracing::Level::INFO);

    let subscriber = tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_target(true)
                .with_level(true)
        );

    if let Some(file) = &config.file {
        let file_appender = std::fs::File::create(file).expect("Failed to create log file");
        let subscriber = subscriber.with(
            fmt::layer()
                .with_writer(file_appender)
                .with_ansi(false)
        );
        subscriber.init();
    } else {
        subscriber.init();
    }

    tracing::info!("Logging initialized at {} level", level);
}
