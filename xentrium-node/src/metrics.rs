//! Minimal Prometheus-style /metrics HTTP endpoint.
//!
//! Serves plain-text metrics over HTTP on a dedicated port — no web
//! framework, just a Tokio TCP listener that answers any request with
//! the current snapshot of node metrics.

use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tracing::{info, warn};

use xentrium_consensus::ValidatorManager;
use xentrium_network::{BasicSnapshotSync, HeartbeatTracker, SnapshotSync, TcpConnection};
use xentrium_storage::SnapshotRepository;

/// Everything the endpoint needs to sample, shared from the Node.
pub struct MetricsSource {
    pub connections: Arc<RwLock<std::collections::HashMap<std::net::SocketAddr, TcpConnection>>>,
    pub validator_manager: Arc<RwLock<ValidatorManager>>,
    pub snapshot_sync: Arc<RwLock<BasicSnapshotSync>>,
    pub snapshot_repository: Option<Arc<SnapshotRepository>>,
    pub heartbeat_tracker: Arc<RwLock<HeartbeatTracker>>,
    pub started_at: Instant,
}

/// Spawn the metrics listener on `port`. Returns immediately.
pub fn spawn_metrics_server(port: u16, src: MetricsSource, running: Arc<RwLock<bool>>) {
    let src = Arc::new(src);
    tokio::spawn(async move {
        let listener = match TcpListener::bind(("0.0.0.0", port)).await {
            Ok(l) => l,
            Err(e) => {
                warn!("metrics endpoint: failed to bind :{} — {}", port, e);
                return;
            }
        };
        info!("Metrics endpoint listening on 0.0.0.0:{}/metrics", port);
        loop {
            if !*running.read().await {
                break;
            }
            match listener.accept().await {
                Ok((mut stream, _)) => {
                    let src = src.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf).await;
                        let body = render(&src).await;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    });
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
            }
        }
    });
}

fn rss_bytes() -> u64 {
    let pid = sysinfo::Pid::from_u32(std::process::id());
    let mut sys = sysinfo::System::new();
    sys.refresh_process(pid);
    sys.process(pid).map(|p| p.memory()).unwrap_or(0)
}

async fn render(src: &MetricsSource) -> String {
    let peers = src.connections.read().await.len();
    let (validators, committee, committee_epoch) = {
        let vm = src.validator_manager.read().await;
        (
            vm.active_validator_count(),
            vm.get_committee_members().len(),
            vm.state_number(),
        )
    };
    let (snap_state, snaps_local) = {
        let sync = src.snapshot_sync.read().await;
        (sync.get_current_state_number(), sync.local_snapshot_count())
    };
    let snaps_db = match &src.snapshot_repository {
        Some(repo) => repo.count().await.unwrap_or(-1),
        None => -1,
    };
    let rtt = src.heartbeat_tracker.read().await.average_rtt();
    let rtt_ms = rtt.map(|d| d.as_millis() as u64).unwrap_or(0);
    let per_peer = src.heartbeat_tracker.read().await.peer_rtts();
    let mut out = String::with_capacity(1024);
    use std::fmt::Write;
    let _ = writeln!(out, "xentrium_peers {}", peers);
    let _ = writeln!(out, "xentrium_validators_active {}", validators);
    let _ = writeln!(out, "xentrium_committee_size {}", committee);
    let _ = writeln!(out, "xentrium_committee_epoch {}", committee_epoch);
    let _ = writeln!(out, "xentrium_snapshot_state {}", snap_state);
    let _ = writeln!(out, "xentrium_snapshots_local {}", snaps_local);
    let _ = writeln!(out, "xentrium_snapshots_db {}", snaps_db);
    let _ = writeln!(out, "xentrium_uptime_seconds {}", src.started_at.elapsed().as_secs());
    let _ = writeln!(out, "xentrium_rss_bytes {}", rss_bytes());
    let _ = writeln!(out, "xentrium_heartbeat_rtt_avg_ms {}", rtt_ms);
    for (peer, d) in per_peer {
        let _ = writeln!(
            out,
            "xentrium_heartbeat_rtt_ms{{peer=\"{}\"}} {}",
            peer.endpoint, d.as_millis() as u64
        );
    }
    out
}
