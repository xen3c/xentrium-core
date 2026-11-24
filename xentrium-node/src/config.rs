use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Node configuration loaded from config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub node: NodeSection,
    pub network: NetworkSection,
    pub consensus: ConsensusSection,
    pub logging: LoggingSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSection {
    pub node_id: String,
    pub address: String,
    pub port: u16,
    pub data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSection {
    pub bootstrap_peers: Vec<String>,
    pub max_peers: usize,
    pub heartbeat_interval_secs: u64,
    pub connection_timeout_secs: u64,
    /// Allowed peer node_ids (pubkey-bound). Empty = open testnet.
    #[serde(default)]
    pub allowed_node_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusSection {
    pub auto_committee_rotation: bool,
    pub snapshot_interval_hours: u64,
    /// Self-register this node's identity as an active validator on startup.
    #[serde(default = "default_true")]
    pub self_register_validator: bool,
    /// Seconds between self-announcements (validator re-announce + PeerList
    /// gossip + cached-state re-serve). Keeps signed envelopes fresh.
    #[serde(default = "default_announce_interval_secs")]
    pub announce_interval_secs: u64,
    /// Committee epoch length: the committee rotates deterministically
    /// every this many seconds.
    #[serde(default = "default_committee_epoch_secs")]
    pub committee_epoch_secs: u64,
    /// Snapshot epoch length: the committee leader produces one snapshot
    /// per epoch and advertises it for peers to pull.
    #[serde(default = "default_snapshot_epoch_secs")]
    pub snapshot_epoch_secs: u64,
    /// Max committee members — deterministic rotation picks a capped
    /// subset of the validator set. Must match fleet-wide.
    #[serde(default = "default_committee_max_size")]
    pub committee_max_size: usize,
    /// Keep latest N snapshots in memory + storage (0 = unbounded).
    #[serde(default = "default_snapshot_retention")]
    pub snapshot_retention: usize,
    /// Prometheus /metrics HTTP port (0 = disabled).
    #[serde(default)]
    pub metrics_port: u16,
}

fn default_true() -> bool { true }
fn default_announce_interval_secs() -> u64 { 60 }
fn default_committee_epoch_secs() -> u64 { 86400 }
fn default_snapshot_epoch_secs() -> u64 { 86400 }
fn default_committee_max_size() -> usize { xentrium_shared::MAX_COMMITTEE_SIZE }
fn default_snapshot_retention() -> usize { 10 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSection {
    pub level: String,
    pub file: Option<String>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node: NodeSection::default(),
            network: NetworkSection::default(),
            consensus: ConsensusSection::default(),
            logging: LoggingSection::default(),
        }
    }
}

impl Default for NodeSection {
    fn default() -> Self {
        Self {
            node_id: "xentrium-node-1".to_string(),
            address: "0.0.0.0".to_string(),
            port: 30303,
            data_dir: "./data".to_string(),
        }
    }
}

impl Default for NetworkSection {
    fn default() -> Self {
        Self {
            bootstrap_peers: vec![],
            max_peers: 50,
            heartbeat_interval_secs: 30,
            connection_timeout_secs: 120,
            allowed_node_ids: vec![],
        }
    }
}

impl Default for ConsensusSection {
    fn default() -> Self {
        Self {
            auto_committee_rotation: true,
            snapshot_interval_hours: 24,
            self_register_validator: true,
            announce_interval_secs: default_announce_interval_secs(),
            committee_epoch_secs: default_committee_epoch_secs(),
            snapshot_epoch_secs: default_snapshot_epoch_secs(),
            committee_max_size: default_committee_max_size(),
            snapshot_retention: default_snapshot_retention(),
            metrics_port: 0,
        }
    }
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file: None,
        }
    }
}

impl NodeConfig {
    /// Load configuration from a TOML file
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: NodeConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration from file or use defaults
    pub fn load_or_default(path: Option<&str>) -> Self {
        if let Some(p) = path {
            if PathBuf::from(p).exists() {
                match Self::load_from_file(p) {
                    Ok(config) => {
                        tracing::info!("Loaded configuration from {}", p);
                        return config;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load config from {}: {}, using defaults", p, e);
                    }
                }
            }
        }
        tracing::info!("Using default configuration");
        Self::default()
    }

    /// Get the full network address
    pub fn network_address(&self) -> String {
        format!("{}:{}", self.node.address, self.node.port)
    }

    /// Get the data directory path
    pub fn data_directory(&self) -> String {
        self.node.data_dir.clone()
    }
}
