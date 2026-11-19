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
}

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
