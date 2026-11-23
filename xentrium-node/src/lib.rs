pub mod config;

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use xentrium_consensus::ValidatorManager;
use xentrium_network::{BasicDiscovery, BasicGossip, BasicSnapshotSync, Discovery, TcpTransport, TcpConnection, PeerID, Peer, PeerState, Message, MessageType, NetworkConnection, SnapshotSync, HeartbeatTracker, NodeIdentity, SignedAnnouncement, ReplayGuard, TlsCredentials, TlsTransportConfig, tls_dir};
use xentrium_storage::{Database, ValidatorRepository, SnapshotRepository, CommitteeRepository, PeerRepository};
use xentrium_shared::{Result, XentriumError, ValidatorInfo, ValidatorState, Snapshot, MAX_COMMITTEE_SIZE};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

/// Node configuration
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NodeConfig {
    pub node_id: String,
    pub network_address: String,
    pub data_directory: String,
    pub bootstrap_peers: Vec<String>,
    pub max_peers: usize,
    pub log_level: String,
    pub heartbeat_interval_secs: u64,
    pub peer_timeout_secs: u64,
    /// Allowed peer node_ids (pubkey-bound identities). Empty = allow all (open testnet).
    pub allowed_node_ids: Vec<String>,
    /// Self-register this node's identity as an active validator on startup.
    #[serde(default)]
    pub self_register_validator: bool,
    /// Seconds between self-announcements (validator re-announce + PeerList
    /// gossip + cached-state re-serve).
    #[serde(default = "default_announce_interval_secs")]
    pub announce_interval_secs: u64,
    /// Committee epoch length in seconds — committee rotates deterministically each epoch.
    #[serde(default = "default_committee_epoch_secs")]
    pub committee_epoch_secs: u64,
    /// Snapshot epoch length in seconds — leader produces one snapshot per epoch.
    #[serde(default = "default_snapshot_epoch_secs")]
    pub snapshot_epoch_secs: u64,
}

fn default_announce_interval_secs() -> u64 { 60 }
fn default_committee_epoch_secs() -> u64 { 86400 }
fn default_snapshot_epoch_secs() -> u64 { 86400 }

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node_id: "xentrium-node-1".to_string(),
            network_address: "127.0.0.1:30303".to_string(),
            data_directory: "./data".to_string(),
            bootstrap_peers: vec![],
            max_peers: 50,
            log_level: "info".to_string(),
            heartbeat_interval_secs: 30,
            peer_timeout_secs: 120,
            allowed_node_ids: vec![],
            self_register_validator: false,
            announce_interval_secs: default_announce_interval_secs(),
            committee_epoch_secs: default_committee_epoch_secs(),
            snapshot_epoch_secs: default_snapshot_epoch_secs(),
        }
    }
}

/// Deterministically derive the committee for an epoch from the active
/// validator set: sort addresses, rotate by epoch, cap at MAX_COMMITTEE_SIZE.
/// Every node with the same validator set computes the identical committee.
pub fn derive_committee(mut validators: Vec<String>, epoch: u64) -> Vec<String> {
    validators.sort();
    validators.dedup();
    if validators.is_empty() {
        return validators;
    }
    let rot = (epoch as usize) % validators.len();
    validators.rotate_left(rot);
    validators.truncate(MAX_COMMITTEE_SIZE);
    validators
}

/// Node runtime
pub struct Node {
    config: NodeConfig,
    validator_manager: Arc<RwLock<ValidatorManager>>,
    discovery: Arc<RwLock<BasicDiscovery>>,
    gossip: Arc<RwLock<BasicGossip>>,
    snapshot_sync: Arc<RwLock<BasicSnapshotSync>>,
    transport: Arc<RwLock<TcpTransport>>,
    connections: Arc<RwLock<HashMap<std::net::SocketAddr, TcpConnection>>>,
    running: Arc<RwLock<bool>>,
    pong_received: Arc<AtomicBool>,
    database: Option<Arc<Database>>,
    validator_repository: Option<Arc<ValidatorRepository>>,
    snapshot_repository: Option<Arc<SnapshotRepository>>,
    committee_repository: Option<Arc<CommitteeRepository>>,
    peer_repository: Option<Arc<PeerRepository>>,
    heartbeat_tracker: Arc<RwLock<HeartbeatTracker>>,
    identity: NodeIdentity,
    /// Replay protection for signed messages (H3)
    replay_guard: Arc<std::sync::Mutex<ReplayGuard>>,
    /// Verified peer announcements available for gossip (H2)
    known_announcements: Arc<RwLock<HashMap<String, SignedAnnouncement>>>,
    /// Verified peer identities (node_id -> public key) learned from
    /// handshakes and announcements; used to authenticate relayed envelopes.
    known_identities: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    /// Validator address -> latest raw signed registration envelope
    /// (served to newly connected peers and relayed on first sight).
    validator_envelopes: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    /// Latest committee selection envelope seen (state_number, raw payload).
    committee_envelope: Arc<RwLock<Option<(i64, Vec<u8>)>>>,
}

impl Node {
    /// Create a new node with the given configuration
    pub fn new(config: NodeConfig) -> Result<Self> {
        info!("Creating node: {}", config.node_id);

        // Initialize validator manager
        let validator_manager = Arc::new(RwLock::new(ValidatorManager::new()));

        // Initialize network services
        let discovery = Arc::new(RwLock::new(BasicDiscovery::new()));
        let gossip = Arc::new(RwLock::new(BasicGossip::new()));
        let snapshot_sync = Arc::new(RwLock::new(BasicSnapshotSync::new(0)));
        let transport = Arc::new(RwLock::new(TcpTransport::new()));
        let connections: Arc<RwLock<HashMap<std::net::SocketAddr, TcpConnection>>> = Arc::new(RwLock::new(HashMap::new()));
        let pong_received = Arc::new(AtomicBool::new(false));

        // Initialize storage (optional for tests)
        let database = None;
        let validator_repository = None;
        let snapshot_repository = None;
        let committee_repository = None;
        let peer_repository = None;
        let heartbeat_tracker = Arc::new(RwLock::new(
            HeartbeatTracker::new(config.heartbeat_interval_secs, config.peer_timeout_secs)
        ));
        let identity = NodeIdentity::load_or_create(
            &std::path::Path::new(&config.data_directory).join("node.identity")
        )?;

        // Seed own signed announcement for gossip
        let mut known_announcements = HashMap::new();
        let own_ann = identity.sign_announcement(&config.network_address);
        known_announcements.insert(identity.node_id.clone(), own_ann);
        let mut known_identities = HashMap::new();
        known_identities.insert(identity.node_id.clone(), identity.public_key_bytes());

        Ok(Self {
            config,
            validator_manager,
            discovery,
            gossip,
            snapshot_sync,
            transport,
            connections,
            running: Arc::new(RwLock::new(false)),
            pong_received,
            database,
            validator_repository,
            snapshot_repository,
            committee_repository,
            peer_repository,
            heartbeat_tracker,
            identity,
            replay_guard: Arc::new(std::sync::Mutex::new(ReplayGuard::new(10_000))),
            known_announcements: Arc::new(RwLock::new(known_announcements)),
            known_identities: Arc::new(RwLock::new(known_identities)),
            validator_envelopes: Arc::new(RwLock::new(HashMap::new())),
            committee_envelope: Arc::new(RwLock::new(None)),
        })
    }

    /// Create a new node with storage enabled
    pub async fn new_with_storage(config: NodeConfig) -> Result<Self> {
        info!("Creating node with storage: {}", config.node_id);

        // Initialize validator manager
        let validator_manager = Arc::new(RwLock::new(ValidatorManager::new()));

        // Initialize network services
        let discovery = Arc::new(RwLock::new(BasicDiscovery::new()));
        let gossip = Arc::new(RwLock::new(BasicGossip::new()));
        let snapshot_sync = Arc::new(RwLock::new(BasicSnapshotSync::new(0)));
        let transport = Arc::new(RwLock::new(TcpTransport::new()));
        let connections: Arc<RwLock<HashMap<std::net::SocketAddr, TcpConnection>>> = Arc::new(RwLock::new(HashMap::new()));
        let pong_received = Arc::new(AtomicBool::new(false));

        // Ensure the data directory exists before SQLite touches it
        std::fs::create_dir_all(&config.data_directory)
            .map_err(|e| XentriumError::DatabaseError(
                format!("Failed to create data directory: {}", e)))?;

        // Initialize storage - use file-based SQLite with WAL mode for better concurrency
        let db_path = format!("{}/xentrium.db", config.data_directory);
        let db_url = format!("sqlite:{}?mode=rwc", db_path);
        let database = Arc::new(Database::new(&db_url).await?);
        let validator_repository = Arc::new(ValidatorRepository::new(database.pool().clone()));
        let snapshot_repository = Arc::new(SnapshotRepository::new(database.pool().clone()));
        let committee_repository = Arc::new(CommitteeRepository::new(database.pool().clone()));
        let peer_repository = Arc::new(PeerRepository::new(database.pool().clone()));
        let heartbeat_tracker = Arc::new(RwLock::new(
            HeartbeatTracker::new(config.heartbeat_interval_secs, config.peer_timeout_secs)
        ));
        let identity = NodeIdentity::load_or_create(
            &std::path::Path::new(&config.data_directory).join("node.identity")
        )?;

        // Seed own signed announcement for gossip
        let mut known_announcements = HashMap::new();
        let own_ann = identity.sign_announcement(&config.network_address);
        known_announcements.insert(identity.node_id.clone(), own_ann);
        let mut known_identities = HashMap::new();
        known_identities.insert(identity.node_id.clone(), identity.public_key_bytes());

        Ok(Self {
            config,
            validator_manager,
            discovery,
            gossip,
            snapshot_sync,
            transport,
            connections,
            running: Arc::new(RwLock::new(false)),
            pong_received,
            database: Some(database),
            validator_repository: Some(validator_repository),
            snapshot_repository: Some(snapshot_repository),
            committee_repository: Some(committee_repository),
            peer_repository: Some(peer_repository),
            heartbeat_tracker,
            identity,
            replay_guard: Arc::new(std::sync::Mutex::new(ReplayGuard::new(10_000))),
            known_announcements: Arc::new(RwLock::new(known_announcements)),
            known_identities: Arc::new(RwLock::new(known_identities)),
            validator_envelopes: Arc::new(RwLock::new(HashMap::new())),
            committee_envelope: Arc::new(RwLock::new(None)),
        })
    }

    /// Start the node
    pub async fn start(&self) -> Result<()> {
        info!("Starting node: {}", self.config.node_id);

        // Initialize database if storage is enabled
        if let Some(ref db) = self.database {
            db.initialize_schema().await?;
            info!("Database schema initialized");
            
            // Load persisted state
            self.load_persisted_state().await?;
        }

        // Set running state
        {
            let mut running = self.running.write().await;
            *running = true;
        }

        // Start TCP listener
        self.start_transport().await?;

        // Initialize network discovery
        self.initialize_discovery().await?;

        // Connect to bootstrap peers
        self.connect_to_bootstrap_peers().await?;

        // Initialize gossip protocol
        self.initialize_gossip().await?;

        // Initialize snapshot sync
        self.initialize_snapshot_sync().await?;

        // Initialize validator manager
        self.initialize_validator_manager().await?;

        // Start background networking tasks
        self.start_networking_tasks().await?;

        // Self-register as validator and announce to already-connected peers
        self.autonomous_bootstrap().await?;

        info!("Node started successfully: {}", self.config.node_id);
        Ok(())
    }

    /// ValidatorInfo describing this node's own identity as a validator.
    fn self_validator_info(&self) -> ValidatorInfo {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        ValidatorInfo {
            address: self.identity.node_id.clone(),
            public_key: self.identity.public_key_bytes(),
            machine_id: self.config.node_id.clone(),
            balance: 0,
            state: ValidatorState::Active,
            enabled_at: Some(now),
            uptime_score: 100,
            reputation_score: 0,
            validator_weight: 0.0,
            last_seen: now,
            consecutive_uptime_weeks: 0,
            committees_participated: 0,
            votes_cast: 0,
            votes_correct: 0,
            endpoint: self.config.network_address.clone(),
            is_active: true,
        }
    }

    /// Sign and broadcast a fresh validator registration envelope, caching the
    /// raw payload so it can be served to peers that connect later.
    async fn announce_validator(&self, validator: &ValidatorInfo) {
        let Ok(inner) = Message::serialize_validator_registration(validator) else {
            return;
        };
        let envelope = self.identity.sign_envelope("xnt-validator-registration", &inner);
        let Ok(payload) = Message::serialize_signed_envelope(&envelope) else {
            return;
        };
        self.validator_envelopes.write().await
            .insert(validator.address.clone(), payload.clone());
        let msg = Message::new(
            MessageType::ValidatorRegistration,
            self.config.node_id.clone().into_bytes(),
            payload,
        );
        let _ = self.broadcast_message(msg).await;
    }

    /// Autonomous startup: self-register as an active validator, persist, and
    /// announce to any peers already connected.
    async fn autonomous_bootstrap(&self) -> Result<()> {
        if !self.config.self_register_validator {
            return Ok(());
        }
        let validator = self.self_validator_info();
        {
            let mut vm = self.validator_manager.write().await;
            let _ = vm.register_validator_from_network(validator.clone());
        }
        let _ = self.persist_validator(&validator).await;
        self.announce_validator(&validator).await;
        info!("Self-registered as validator: {}", validator.address);
        Ok(())
    }

    /// Stop the node gracefully
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping node: {}", self.config.node_id);

        // Set running state to false
        {
            let mut running = self.running.write().await;
            *running = false;
        }

        // Close all connections
        {
            let mut connections = self.connections.write().await;
            connections.clear();
        }

        // Release the listening socket so a restarted node can rebind
        {
            let mut transport = self.transport.write().await;
            transport.stop_listener();
        }

        // Close database connection if storage is enabled
        // This is done by dropping the Arc reference
        drop(self.database.clone());
        drop(self.validator_repository.clone());
        drop(self.snapshot_repository.clone());

        info!("Node stopped successfully: {}", self.config.node_id);
        Ok(())
    }

    /// Check if the node is running
    pub async fn is_running(&self) -> bool {
        let running = self.running.read().await;
        *running
    }

    /// Get the node configuration
    pub fn config(&self) -> &NodeConfig {
        &self.config
    }

    /// Get validator manager reference
    pub fn validator_manager(&self) -> Arc<RwLock<ValidatorManager>> {
        self.validator_manager.clone()
    }

    /// Get discovery reference
    pub fn discovery(&self) -> Arc<RwLock<BasicDiscovery>> {
        self.discovery.clone()
    }

    /// Get gossip reference
    pub fn gossip(&self) -> Arc<RwLock<BasicGossip>> {
        self.gossip.clone()
    }

    /// Get snapshot sync reference
    pub fn snapshot_sync(&self) -> Arc<RwLock<BasicSnapshotSync>> {
        self.snapshot_sync.clone()
    }

    /// Get heartbeat tracker reference
    pub fn heartbeat_tracker(&self) -> Arc<RwLock<HeartbeatTracker>> {
        self.heartbeat_tracker.clone()
    }

    /// Get node identity
    pub fn identity(&self) -> &NodeIdentity {
        &self.identity
    }

    /// Number of verified peer announcements currently known (for gossip/tests)
    pub async fn known_announcement_count(&self) -> usize {
        self.known_announcements.read().await.len()
    }

    /// Get transport reference
    pub fn transport(&self) -> Arc<RwLock<TcpTransport>> {
        self.transport.clone()
    }

    /// Check if a Pong message was received
    pub fn pong_received(&self) -> bool {
        self.pong_received.load(Ordering::SeqCst)
    }

    /// Reset pong received flag
    pub fn reset_pong_received(&self) {
        self.pong_received.store(false, Ordering::SeqCst);
    }

    /// Get current connections (for debugging)
    pub async fn connections(&self) -> Vec<std::net::SocketAddr> {
        let connections = self.connections.read().await;
        connections.keys().cloned().collect()
    }

    /// Send a message to a specific peer
    pub async fn send_message(&self, addr: &std::net::SocketAddr, message: Message) -> Result<()> {
        info!("Attempting to send message to {:?}", addr);
        let mut connections = self.connections.write().await;
        info!("Current connections: {:?}", connections.keys().collect::<Vec<_>>());
        if let Some(connection) = connections.get_mut(addr) {
            info!("Found connection for {:?}, sending message", addr);
            connection.send(message).await
                .map_err(|e| XentriumError::DatabaseError(format!("Failed to send message: {}", e)))?;
            info!("Message sent successfully to {:?}", addr);
            Ok(())
        } else {
            info!("No connection found for {:?}", addr);
            Err(XentriumError::DatabaseError("Peer not connected".to_string()))
        }
    }

    /// Send a message to all connected peers
    pub async fn broadcast_message(&self, message: Message) -> Result<()> {
        let mut connections = self.connections.write().await;
        for (_, connection) in connections.iter_mut() {
            let _ = connection.send(message.clone()).await;
        }
        Ok(())
    }

    /// Broadcast a PeerList message containing signed peer announcements (H2)
    pub async fn broadcast_peer_list(&self) -> Result<()> {
        // Collect verified announcements (own + learned from peers), capped for gossip
        let announcements: Vec<SignedAnnouncement> = {
            let known = self.known_announcements.read().await;
            known.values().take(64).cloned().collect()
        };

        let payload = Message::serialize_announcements(&announcements)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize announcements: {}", e)))?;

        let peerlist_msg = Message::new(
            MessageType::PeerList,
            self.config.node_id.clone().into_bytes(),
            payload,
        );

        self.broadcast_message(peerlist_msg).await
    }

    /// Broadcast a signed validator registration to all connected peers (C3)
    pub async fn broadcast_validator_registration(&self, validator: xentrium_shared::ValidatorInfo) -> Result<()> {
        let inner = Message::serialize_validator_registration(&validator)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize validator: {}", e)))?;
        let envelope = self.identity.sign_envelope("xnt-validator-registration", &inner);
        let payload = Message::serialize_signed_envelope(&envelope)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize envelope: {}", e)))?;

        let msg = Message::new(
            MessageType::ValidatorRegistration,
            self.config.node_id.clone().into_bytes(),
            payload,
        );

        self.broadcast_message(msg).await
    }

    /// Broadcast a signed committee selection to all connected peers (C3)
    pub async fn broadcast_committee_selection(&self, members: Vec<String>, state_number: i64) -> Result<()> {
        let inner = Message::serialize_committee_selection(&members, state_number)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize committee: {}", e)))?;
        let envelope = self.identity.sign_envelope("xnt-committee-selection", &inner);
        let payload = Message::serialize_signed_envelope(&envelope)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize envelope: {}", e)))?;

        let msg = Message::new(
            MessageType::CommitteeSelection,
            self.config.node_id.clone().into_bytes(),
            payload,
        );

        self.broadcast_message(msg).await
    }

    /// Broadcast a snapshot offer to all connected peers
    pub async fn broadcast_snapshot_offer(&self, snapshot: &xentrium_shared::Snapshot) -> Result<()> {
        let payload = Message::serialize_snapshot_metadata(snapshot)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize snapshot metadata: {}", e)))?;
        
        let msg = Message::new(
            MessageType::SnapshotOffer,
            self.config.node_id.clone().into_bytes(),
            payload,
        );
        
        self.broadcast_message(msg).await
    }

    /// Send snapshot data to a specific peer
    pub async fn send_snapshot_data(&self, snapshot: &xentrium_shared::Snapshot, peer_addr: std::net::SocketAddr) -> Result<()> {
        let payload = Message::serialize_snapshot(snapshot)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize snapshot: {}", e)))?;
        
        let msg = Message::new(
            MessageType::SnapshotData,
            self.config.node_id.clone().into_bytes(),
            payload,
        );
        
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(&peer_addr) {
            connection.send(msg).await
                .map_err(|e| XentriumError::DatabaseError(format!("Failed to send snapshot: {}", e)))?;
            Ok(())
        } else {
            Err(XentriumError::DatabaseError("Peer not connected".to_string()))
        }
    }

    /// Broadcast snapshot data to all connected peers
    pub async fn broadcast_snapshot_data(&self, snapshot: &xentrium_shared::Snapshot) -> Result<()> {
        let payload = Message::serialize_snapshot(snapshot)
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to serialize snapshot: {}", e)))?;
        
        let msg = Message::new(
            MessageType::SnapshotData,
            self.config.node_id.clone().into_bytes(),
            payload,
        );
        
        self.broadcast_message(msg).await
    }

    /// Load persisted state from storage
    async fn load_persisted_state(&self) -> Result<()> {
        if let (Some(ref validator_repo), Some(ref snapshot_repo), Some(ref committee_repo), Some(ref peer_repo)) = 
            (&self.validator_repository, &self.snapshot_repository, &self.committee_repository, &self.peer_repository) {
            
            // Load validators
            if let Ok(validators) = validator_repo.get_all().await {
                let count = validators.len();
                let mut vm = self.validator_manager.write().await;
                for validator in validators {
                    let _ = vm.register_validator_from_network(validator);
                }
                if count > 0 {
                    info!("Loaded {} validators from storage", count);
                }
            }

            // Load latest snapshot
            if let Ok(Some(snapshot)) = snapshot_repo.get_latest().await {
                let state_number = snapshot.state_number;
                let mut sync = self.snapshot_sync.write().await;
                sync.set_current_state_number(state_number);
                let _ = sync.serve_snapshot(snapshot);
                info!("Loaded snapshot from storage: state {}", state_number);
            }

            // Load committee
            if let Ok(committee_members) = committee_repo.get().await {
                if !committee_members.is_empty() {
                    let mut vm = self.validator_manager.write().await;
                    let state_number = committee_repo.get_state_number().await.unwrap_or(0);
                    let _ = vm.update_committee(&committee_members, state_number);
                    info!("Loaded committee from storage: {} members", committee_members.len());
                }
            }

            // Load peers
            if let Ok(stored_peers) = peer_repo.get_all().await {
                let peer_count = stored_peers.len();
                let mut discovery = self.discovery.write().await;
                for peer in &stored_peers {
                    // Parse endpoint as SocketAddr
                    let socket_addr: std::net::SocketAddr = peer.endpoint.parse()
                        .unwrap_or_else(|_| "127.0.0.1:30303".parse().unwrap());
                    
                    let peer_id = PeerID::new(
                        peer.address.clone(),
                        socket_addr,
                        peer.machine_id.clone(),
                    );
                    
                    let peer_state = match peer.state.as_str() {
                        "Connected" => PeerState::Connected,
                        "Disconnected" => PeerState::Disconnected,
                        "Banned" => PeerState::Banned,
                        _ => PeerState::Connecting,
                    };
                    
                    let mut p = Peer::new(peer_id);
                    p.set_state(peer_state);
                    p.uptime_seconds = peer.uptime_score as u64;
                    p.reputation_score = peer.uptime_score as i32;
                    
                    let _ = discovery.add_peer(p);
                    
                    if peer.is_bootstrap {
                        let _ = discovery.add_bootstrap_peer(socket_addr);
                    }
                }
                info!("Loaded {} peers from storage", peer_count);
            }
        }
        Ok(())
    }

    /// Persist validator to storage
    pub async fn persist_validator(&self, validator: &ValidatorInfo) -> Result<()> {
        if let Some(ref validator_repo) = self.validator_repository {
            validator_repo.register(validator).await?;
            info!("Persisted validator: {}", validator.address);
        }
        Ok(())
    }

    /// Persist snapshot to storage
    pub async fn persist_snapshot(&self, snapshot: &Snapshot) -> Result<()> {
        if let Some(ref snapshot_repo) = self.snapshot_repository {
            snapshot_repo.store(snapshot).await?;
            info!("Persisted snapshot: state {}", snapshot.state_number);
        }
        Ok(())
    }

    /// Persist committee to storage
    pub async fn persist_committee(&self, members: &[String], state_number: i64) -> Result<()> {
        if let Some(ref committee_repo) = self.committee_repository {
            committee_repo.store(members, state_number).await?;
            info!("Persisted committee: {} members", members.len());
        }
        Ok(())
    }

    /// Persist peer to storage
    pub async fn persist_peer(&self, peer: &xentrium_storage::StoredPeer) -> Result<()> {
        if let Some(ref peer_repo) = self.peer_repository {
            peer_repo.store(peer).await?;
            info!("Persisted peer: {}", peer.address);
        }
        Ok(())
    }

    /// Get database reference (for testing)
    pub fn database(&self) -> Option<Arc<Database>> {
        self.database.clone()
    }

    /// Get validator repository reference (for testing)
    pub fn validator_repository(&self) -> Option<Arc<ValidatorRepository>> {
        self.validator_repository.clone()
    }

    /// Get snapshot repository reference (for testing)
    pub fn snapshot_repository(&self) -> Option<Arc<SnapshotRepository>> {
        self.snapshot_repository.clone()
    }

    /// Get committee repository reference (for testing)
    pub fn committee_repository(&self) -> Option<Arc<CommitteeRepository>> {
        self.committee_repository.clone()
    }

    /// Get peer repository reference (for testing)
    pub fn peer_repository(&self) -> Option<Arc<PeerRepository>> {
        self.peer_repository.clone()
    }

    /// Initialize network discovery
    async fn initialize_discovery(&self) -> Result<()> {
        info!("Initializing network discovery");

        let mut discovery = self.discovery.write().await;

        // Add bootstrap peers
        for peer_addr in &self.config.bootstrap_peers {
            let addr = peer_addr.parse::<std::net::SocketAddr>()
                .map_err(|e| XentriumError::DatabaseError(format!("Invalid peer address: {}", e)))?;
            // Direct method call instead of trait
            let _ = discovery.add_bootstrap_peer(addr);
        }

        info!("Network discovery initialized with {} bootstrap peers", self.config.bootstrap_peers.len());
        Ok(())
    }

    /// Start TCP transport listener
    async fn start_transport(&self) -> Result<()> {
        info!("Starting TCP transport");

        let addr = self.config.network_address.parse::<std::net::SocketAddr>()
            .map_err(|e| XentriumError::DatabaseError(format!("Invalid network address: {}", e)))?;

        let mut transport = self.transport.write().await;
        transport.start_listener(addr).await
            .map_err(|e| XentriumError::DatabaseError(format!("Failed to start listener: {}", e)))?;

        // Enable TLS transport with identity-bound certificate
        let creds = TlsCredentials::load_or_generate(
            &tls_dir(&self.config.data_directory),
            &self.identity.node_id,
        ).map_err(|e| XentriumError::DatabaseError(format!("TLS credentials: {}", e)))?;
        let tls_config = TlsTransportConfig::new(&creds)
            .map_err(|e| XentriumError::DatabaseError(format!("TLS config: {}", e)))?;
        transport.enable_tls(tls_config);
        info!("TLS transport enabled (cert CN = {})", self.identity.node_id);

        info!("TCP transport started on {}", addr);
        Ok(())
    }

    /// Connect to bootstrap peers
    async fn connect_to_bootstrap_peers(&self) -> Result<()> {
        info!("Connecting to bootstrap peers: {:?}", self.config.bootstrap_peers);

        let transport = self.transport.read().await;
        let mut discovery = self.discovery.write().await;
        let mut connections = self.connections.write().await;

        for peer_addr in &self.config.bootstrap_peers {
            let addr = peer_addr.parse::<std::net::SocketAddr>()
                .map_err(|e| XentriumError::DatabaseError(format!("Invalid peer address: {}", e)))?;

            let peer_id = PeerID::new(
                addr.to_string(),
                addr,
                self.config.node_id.clone(),
            );

            match tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                transport.connect(peer_id.clone())
            ).await {
                Ok(connect_result) => match connect_result {
                    Ok(mut connection) => {
                        info!("Connected to bootstrap peer: {}", addr);
                        // Send signed handshake
                        let hs = self.identity.create_handshake(&self.config.network_address);
                        if let Ok(payload) = Message::serialize_handshake(&hs) {
                            let _ = connection.send(Message::new(
                                MessageType::Handshake,
                                self.identity.node_id.clone().into_bytes(),
                                payload,
                            )).await;
                        }
                        let mut peer = Peer::new(peer_id.clone());
                        peer.set_state(PeerState::Connected);
                        discovery.add_peer(peer)
                            .map_err(|e| XentriumError::DatabaseError(format!("Failed to add peer: {}", e)))?;
                        connections.insert(addr, connection);
                    }
                    Err(e) => {
                        info!("Failed to connect to bootstrap peer {}: {:?}", addr, e);
                    }
                }
                Err(_) => {
                    info!("Timed out connecting to bootstrap peer {}", addr);
                }
            }
        }

        Ok(())
    }

    /// Handle an incoming message (dispatch layer)
    async fn handle_message(&self, peer_id: &PeerID, message: Message, connection: &mut TcpConnection) -> Result<()> {
        match message.message_type {
            MessageType::Ping => {
                // Send Pong response
                let pong = Message::new(
                    MessageType::Pong,
                    self.config.node_id.clone().into_bytes(),
                    b"PONG".to_vec(),
                );
                connection.send(pong).await
                    .map_err(|e| XentriumError::DatabaseError(format!("Failed to send Pong: {}", e)))?;
                info!("Sent Pong to {:?}", peer_id.endpoint);
            }
            MessageType::Pong => {
                // Set Pong received flag
                self.pong_received.store(true, Ordering::SeqCst);
                info!("Received Pong from {:?}", peer_id.endpoint);
            }
            MessageType::PeerList => {
                // Process PeerList through Discovery
                let mut discovery = self.discovery.write().await;
                // For now, deserialize peer list from payload
                // In production, this would use proper serialization
                let _ = discovery.receive_peer_list(vec![]);
                info!("Processed PeerList from {:?}", peer_id.endpoint);
            }
            _ => {
                info!("Unhandled message type: {:?}", message.message_type);
            }
        }
        Ok(())
    }

    /// Start background networking tasks
    async fn start_networking_tasks(&self) -> Result<()> {
        info!("Starting background networking tasks");

        // Create all clones upfront (async move tasks consume them)
        let running_a = self.running.clone();
        let transport_a = self.transport.clone();
        let connections_a = self.connections.clone();
        let discovery_a = self.discovery.clone();
        let identity_a = self.identity.clone();

        let running_b = self.running.clone();
        let connections_b = self.connections.clone();
        let discovery_b = self.discovery.clone();
        let validator_manager_b = self.validator_manager.clone();
        let snapshot_sync_b = self.snapshot_sync.clone();
        let node_id_b = self.config.node_id.clone();
        let pong_received_b = self.pong_received.clone();
        let heartbeat_tracker_b = self.heartbeat_tracker.clone();
        let replay_guard_b = self.replay_guard.clone();
        let known_announcements_b = self.known_announcements.clone();
        let allowed_node_ids_b = self.config.allowed_node_ids.clone();
        let known_identities_b = self.known_identities.clone();
        let validator_envelopes_b = self.validator_envelopes.clone();
        let committee_envelope_b = self.committee_envelope.clone();
        let validator_repo_b = self.validator_repository.clone();
        let committee_repo_b = self.committee_repository.clone();
        let snapshot_repo_b = self.snapshot_repository.clone();

        let running_hb = self.running.clone();
        let connections_hb = self.connections.clone();
        let discovery_hb = self.discovery.clone();
        let heartbeat_tracker_hb = self.heartbeat_tracker.clone();
        let node_id_hb = self.config.node_id.clone();
        let heartbeat_interval_hb = self.config.heartbeat_interval_secs;

        let running_pd = self.running.clone();
        let discovery_pd = self.discovery.clone();

        let running_gm = self.running.clone();
        let gossip_gm = self.gossip.clone();

        let running_sa = self.running.clone();
        let snapshot_sync_sa = self.snapshot_sync.clone();

        // Autonomous-operation clones
        let running_an = self.running.clone();
        let connections_an = self.connections.clone();
        let identity_an = self.identity.clone();
        let node_id_an = self.config.node_id.clone();
        let known_announcements_an = self.known_announcements.clone();
        let validator_envelopes_an = self.validator_envelopes.clone();
        let committee_envelope_an = self.committee_envelope.clone();
        let snapshot_sync_an = self.snapshot_sync.clone();
        let announce_interval_an = self.config.announce_interval_secs.max(5);
        let self_validator_an = if self.config.self_register_validator {
            Some(self.self_validator_info())
        } else {
            None
        };

        let running_cm = self.running.clone();
        let validator_manager_cm = self.validator_manager.clone();
        let committee_repo_cm = self.committee_repository.clone();
        let committee_envelope_cm = self.committee_envelope.clone();
        let connections_cm = self.connections.clone();
        let identity_cm = self.identity.clone();
        let node_id_cm = self.config.node_id.clone();
        let own_validator_addr_cm = self.identity.node_id.clone();
        let committee_epoch_cm = self.config.committee_epoch_secs.max(1);
        let committee_enabled_cm = self.config.self_register_validator;
        let committee_poll_cm = (self.config.committee_epoch_secs / 4).clamp(2, 30);

        let running_sp = self.running.clone();
        let validator_manager_sp = self.validator_manager.clone();
        let snapshot_sync_sp = self.snapshot_sync.clone();
        let snapshot_repo_sp = self.snapshot_repository.clone();
        let connections_sp = self.connections.clone();
        let node_id_sp = self.config.node_id.clone();
        let own_validator_addr_sp = self.identity.node_id.clone();
        let snapshot_epoch_sp = self.config.snapshot_epoch_secs.max(1);
        let snapshot_enabled_sp = self.config.self_register_validator;
        let snapshot_poll_sp = (self.config.snapshot_epoch_secs / 4).clamp(2, 30);

        // Spawn accept loop task
        tokio::spawn(async move {
            loop {
                let is_running = *running_a.read().await;
                if !is_running {
                    break;
                }

                // Time out only the TCP accept so the loop stays responsive to
                // shutdown; the TLS handshake runs in its own task without a
                // short timeout, otherwise healthy WAN handshakes get dropped.
                let accepted = {
                    let transport = transport_a.read().await;
                    tokio::time::timeout(
                        tokio::time::Duration::from_millis(50),
                        transport.accept_tcp()
                    ).await
                };

                match accepted {
                    Ok(Ok((stream, remote_addr))) => {
                        let transport_c = transport_a.clone();
                        let identity_c = identity_a.clone();
                        let discovery_c = discovery_a.clone();
                        let connections_c = connections_a.clone();
                        tokio::spawn(async move {
                            let mut connection = {
                                let transport = transport_c.read().await;
                                match tokio::time::timeout(
                                    tokio::time::Duration::from_secs(30),
                                    transport.upgrade_connection(stream, remote_addr)
                                ).await {
                                    Ok(Ok(connection)) => connection,
                                    Ok(Err(e)) => {
                                        info!("TLS accept failed from {:?}: {:?}", remote_addr, e);
                                        return;
                                    }
                                    Err(_) => {
                                        info!("TLS accept timed out from {:?}", remote_addr);
                                        return;
                                    }
                                }
                            };

                            info!("Accepted connection from: {:?}", remote_addr);

                            // Send signed handshake to accepted peer
                            let hs = identity_c.create_handshake(&remote_addr.to_string());
                            if let Ok(payload) = Message::serialize_handshake(&hs) {
                                let _ = connection.send(Message::new(
                                    MessageType::Handshake,
                                    identity_c.node_id.clone().into_bytes(),
                                    payload,
                                )).await;
                            }

                            let peer_id = connection.peer_id().clone();
                            let mut discovery = discovery_c.write().await;
                            let mut connections = connections_c.write().await;

                            let mut peer = Peer::new(peer_id);
                            peer.set_state(PeerState::Connected);
                            let _ = discovery.add_peer(peer);
                            connections.insert(remote_addr, connection);
                            info!("Accepted and stored connection from remote {:?}", remote_addr);
                        });
                    }
                    Ok(Err(e)) => {
                        // Log and keep accepting — a single bad connection must
                        // not permanently kill the accept loop.
                        info!("Accept error: {:?}", e);
                    }
                    Err(_) => {
                        // Timeout - normal, continue accepting
                    }
                }
            }
        });

        // Spawn bootstrap reconnect task — re-dials any bootstrap peer whose
        // connection was dropped, otherwise a dead link leaves the node
        // isolated with no recovery.
        tokio::spawn({
            let running_rc = self.running.clone();
            let transport_rc = self.transport.clone();
            let discovery_rc = self.discovery.clone();
            let connections_rc = self.connections.clone();
            let identity_rc = self.identity.clone();
            let bootstrap_rc = self.config.bootstrap_peers.clone();
            let network_address_rc = self.config.network_address.clone();
            let node_id_rc = self.config.node_id.clone();
            async move {
                loop {
                    if !*running_rc.read().await {
                        break;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

                    for peer_addr in &bootstrap_rc {
                        let addr = match peer_addr.parse::<std::net::SocketAddr>() {
                            Ok(a) => a,
                            Err(_) => continue,
                        };
                        let already_connected = {
                            let connections = connections_rc.read().await;
                            connections.contains_key(&addr)
                        };
                        if already_connected {
                            continue;
                        }

                        let peer_id = PeerID::new(
                            addr.to_string(),
                            addr,
                            node_id_rc.clone(),
                        );

                        let result = {
                            let transport = transport_rc.read().await;
                            tokio::time::timeout(
                                tokio::time::Duration::from_secs(30),
                                transport.connect(peer_id.clone())
                            ).await
                        };

                        match result {
                            Ok(Ok(mut connection)) => {
                                info!("Reconnected to bootstrap peer: {}", addr);
                                let hs = identity_rc.create_handshake(&network_address_rc);
                                if let Ok(payload) = Message::serialize_handshake(&hs) {
                                    let _ = connection.send(Message::new(
                                        MessageType::Handshake,
                                        node_id_rc.clone().into_bytes(),
                                        payload,
                                    )).await;
                                }
                                let mut discovery = discovery_rc.write().await;
                                let mut connections = connections_rc.write().await;
                                let mut peer = Peer::new(peer_id);
                                peer.set_state(PeerState::Connected);
                                let _ = discovery.add_peer(peer);
                                connections.insert(addr, connection);
                            }
                            Ok(Err(e)) => {
                                info!("Bootstrap reconnect to {} failed: {:?}", addr, e);
                            }
                            Err(_) => {
                                info!("Bootstrap reconnect to {} timed out", addr);
                            }
                        }
                    }
                }
            }
        });

        // Spawn message receive loop task
        tokio::spawn(async move {
            loop {
                let is_running = *running_b.read().await;
                if !is_running {
                    break;
                }

                let mut connections_to_remove = Vec::new();
                
                let addrs: Vec<std::net::SocketAddr> = {
                    let connections = connections_b.read().await;
                    connections.keys().cloned().collect()
                };
                
                for addr in addrs {
                    // Deferred sends: Some(peer) = direct reply, None = relay to
                    // all connections except the source `addr`.
                    let mut outbox: Vec<(Option<std::net::SocketAddr>, Message)> = Vec::new();
                    let mut connections = connections_b.write().await;
                    if let Some(connection) = connections.get_mut(&addr) {
                        match tokio::time::timeout(
                            tokio::time::Duration::from_millis(50),
                            connection.receive()
                        ).await {
                            Ok(Ok(Some(message))) => {
                                info!("Received message from {:?}: {:?}", addr, message.message_type);
                                
                                // Handle Handshake: verify peer identity, reject invalid
                                if message.message_type == MessageType::Handshake {
                                    let hs = Message::deserialize_handshake(&message.payload);
                                    match hs {
                                        Ok(payload) => {
                                            // Bind handshake identity to TLS certificate CN
                                            let cert_mismatch = connection.tls_peer_node_id
                                                .as_ref()
                                                .map(|cn| cn != &payload.node_id)
                                                .unwrap_or(false);
                                            // H1: peer allowlist enforcement
                                            let allowlisted = allowed_node_ids_b.is_empty()
                                                || allowed_node_ids_b.iter().any(|id| id == &payload.node_id);
                                            // H3: replay protection on (node_id, nonce)
                                            let fresh = {
                                                let mut guard = replay_guard_b.lock()
                                                    .unwrap_or_else(|e| e.into_inner());
                                                guard.check_and_insert(&payload.node_id, payload.nonce)
                                            };
                                            match NodeIdentity::verify_handshake(&payload) {
                                                Ok(()) if !cert_mismatch && allowlisted && fresh => {
                                                    info!("Verified handshake from {:?}: node_id={} (TLS-bound)", addr, payload.node_id);
                                                    // Record authenticated identity for signed-message checks
                                                    connection.auth_node_id = Some(payload.node_id.clone());
                                                    connection.auth_pubkey = Some(payload.public_key.clone());
                                                    // Cache peer announcement for gossip relay
                                                    let ann = NodeIdentity::announcement_from_handshake(&payload);
                                                    known_announcements_b.write().await
                                                        .insert(payload.node_id.clone(), ann);
                                                    // Record verified identity so envelopes relayed
                                                    // through third parties can be authenticated
                                                    known_identities_b.write().await
                                                        .insert(payload.node_id.clone(), payload.public_key.clone());

                                                    // Autonomous sync: serve cached network state to
                                                    // the newly verified peer
                                                    let raw_envs: Vec<Vec<u8>> = validator_envelopes_b
                                                        .read().await.values().cloned().collect();
                                                    for raw in raw_envs {
                                                        let _ = connection.send(Message::new(
                                                            MessageType::ValidatorRegistration,
                                                            node_id_b.clone().into_bytes(),
                                                            raw,
                                                        )).await;
                                                    }
                                                    if let Some((_, raw)) = committee_envelope_b
                                                        .read().await.clone()
                                                    {
                                                        let _ = connection.send(Message::new(
                                                            MessageType::CommitteeSelection,
                                                            node_id_b.clone().into_bytes(),
                                                            raw,
                                                        )).await;
                                                    }
                                                    let latest_snap = {
                                                        let sync = snapshot_sync_b.read().await;
                                                        sync.get_latest_local()
                                                    };
                                                    if let Some(snap) = latest_snap {
                                                        if let Ok(meta) = Message::serialize_snapshot_metadata(&snap) {
                                                            let _ = connection.send(Message::new(
                                                                MessageType::SnapshotOffer,
                                                                node_id_b.clone().into_bytes(),
                                                                meta,
                                                            )).await;
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    info!("Rejecting peer {:?}: invalid handshake, cert/identity mismatch, not allowlisted, or replay", addr);
                                                    connections_to_remove.push(addr);
                                                }
                                            }
                                        }
                                        Err(_) => {
                                            info!("Rejecting peer {:?}: malformed handshake", addr);
                                            connections_to_remove.push(addr);
                                        }
                                    }
                                }

                                // Set pong_received flag for Pong messages and track latency
                                if message.message_type == MessageType::Pong {
                                    pong_received_b.store(true, Ordering::SeqCst);
                                    // Track pong latency for the sending peer
                                    let peer_id = PeerID::new(
                                        format!("peer-{}", addr),
                                        addr,
                                        addr.to_string(),
                                    );
                                    let mut tracker = heartbeat_tracker_b.write().await;
                                    if let Some((_, latency)) = tracker.record_pong(&peer_id) {
                                        info!("Heartbeat: Pong from {:?}, latency {:?}", addr, latency);
                                    }
                                }
                                
                                // Handle Ping -> send Pong response
                                if message.message_type == MessageType::Ping {
                                    let pong = Message::new(
                                        MessageType::Pong,
                                        node_id_b.clone().into_bytes(),
                                        b"PONG".to_vec(),
                                    );
                                    let _ = connection.send(pong).await;
                                }
                                
                                // Handle PeerList (signed announcements, H2)
                                if message.message_type == MessageType::PeerList {
                                    info!("Processing PeerList from {:?}", addr);
                                    let anns = Message::deserialize_announcements(&message.payload);
                                    if let Ok(announcements) = anns {
                                        let mut peers = Vec::new();
                                        let mut valid_anns = Vec::new();
                                        for ann in announcements {
                                            // H1: allowlist filter
                                            if !allowed_node_ids_b.is_empty()
                                                && !allowed_node_ids_b.iter().any(|id| id == &ann.node_id) {
                                                continue;
                                            }
                                            // Verify signature + freshness + node_id binding
                                            if NodeIdentity::verify_announcement(&ann).is_err() {
                                                continue;
                                            }
                                            // H3: replay protection
                                            let fresh = {
                                                let mut guard = replay_guard_b.lock()
                                                    .unwrap_or_else(|e| e.into_inner());
                                                guard.check_and_insert(&ann.node_id, ann.nonce)
                                            };
                                            if !fresh {
                                                continue;
                                            }
                                            // Convert to PeerID
                                            if let Ok(endpoint) = ann.endpoint.parse::<std::net::SocketAddr>() {
                                                peers.push(PeerID::new(
                                                    ann.endpoint.clone(),
                                                    endpoint,
                                                    ann.node_id.clone(),
                                                ));
                                                valid_anns.push(ann);
                                            }
                                        }
                                        if !peers.is_empty() {
                                            let mut discovery = discovery_b.write().await;
                                            let added = discovery.receive_peer_list(peers.clone());
                                            info!("PeerList: {} valid announcements, added {} peers", valid_anns.len(), added.unwrap_or(0));
                                            drop(discovery);
                                            // Cache verified announcements for relay and record
                                            // their identities so relayed envelopes authenticate
                                            let mut known = known_announcements_b.write().await;
                                            let mut known_ids = known_identities_b.write().await;
                                            for ann in valid_anns {
                                                known_ids.insert(ann.node_id.clone(), ann.public_key.clone());
                                                known.insert(ann.node_id.clone(), ann);
                                            }
                                        }
                                    }
                                }

                                // Handle ValidatorRegistration (signed envelope, C3)
                                if message.message_type == MessageType::ValidatorRegistration {
                                    info!("Processing ValidatorRegistration from {:?}", addr);
                                    let env = Message::deserialize_signed_envelope(&message.payload);
                                    if let Ok(envelope) = env {
                                        // Verify signature, freshness, node_id binding
                                        let verified = NodeIdentity::verify_envelope(
                                            &envelope, "xnt-validator-registration");
                                        // H3: replay protection
                                        let fresh = verified.is_ok() && {
                                            let mut guard = replay_guard_b.lock()
                                                .unwrap_or_else(|e| e.into_inner());
                                            guard.check_and_insert(&envelope.node_id, envelope.nonce)
                                        };
                                        // C3: envelope identity must match this connection's
                                        // authenticated identity — OR an identity already
                                        // verified via handshake/announcement gossip, which
                                        // permits signed envelopes relayed through the seed.
                                        let direct_match = connection.auth_node_id.as_deref()
                                            == Some(envelope.node_id.as_str())
                                            && connection.auth_pubkey.as_deref()
                                                == Some(envelope.public_key.as_slice());
                                        let known_match = {
                                            let known = known_identities_b.read().await;
                                            known.get(&envelope.node_id)
                                                .map(|pk| pk == &envelope.public_key)
                                                .unwrap_or(false)
                                        };
                                        if fresh && (direct_match || known_match) {
                                            if let Ok(inner) = verified {
                                                let validator = Message::deserialize_validator_registration(&inner);
                                                if let Ok(validator_info) = validator {
                                                    let is_new = !validator_envelopes_b.read().await
                                                        .contains_key(&validator_info.address);
                                                    {
                                                        let mut vm = validator_manager_b.write().await;
                                                        let _ = vm.register_validator_from_network(validator_info.clone());
                                                    }
                                                    validator_envelopes_b.write().await
                                                        .insert(validator_info.address.clone(), message.payload.clone());
                                                    info!("Registered validator: {} (signed by {})", validator_info.address, envelope.node_id);
                                                    if is_new {
                                                        if let Some(ref repo) = validator_repo_b {
                                                            let _ = repo.register(&validator_info).await;
                                                        }
                                                        // Relay so indirectly connected peers learn it
                                                        outbox.push((None, message.clone()));
                                                    }
                                                }
                                            }
                                        } else {
                                            info!("Rejected ValidatorRegistration from {:?}: invalid signature, replay, or identity mismatch", addr);
                                        }
                                    } else {
                                        info!("Rejected ValidatorRegistration from {:?}: malformed envelope (unsigned payloads are not accepted)", addr);
                                    }
                                }

                                // Handle CommitteeSelection (signed envelope, C3)
                                if message.message_type == MessageType::CommitteeSelection {
                                    info!("Processing CommitteeSelection from {:?}", addr);
                                    let env = Message::deserialize_signed_envelope(&message.payload);
                                    if let Ok(envelope) = env {
                                        let verified = NodeIdentity::verify_envelope(
                                            &envelope, "xnt-committee-selection");
                                        let fresh = verified.is_ok() && {
                                            let mut guard = replay_guard_b.lock()
                                                .unwrap_or_else(|e| e.into_inner());
                                            guard.check_and_insert(&envelope.node_id, envelope.nonce)
                                        };
                                        let direct_match = connection.auth_node_id.as_deref()
                                            == Some(envelope.node_id.as_str())
                                            && connection.auth_pubkey.as_deref()
                                                == Some(envelope.public_key.as_slice());
                                        let known_match = {
                                            let known = known_identities_b.read().await;
                                            known.get(&envelope.node_id)
                                                .map(|pk| pk == &envelope.public_key)
                                                .unwrap_or(false)
                                        };
                                        if fresh && (direct_match || known_match) {
                                            if let Ok(inner) = verified {
                                                let committee = Message::deserialize_committee_selection(&inner);
                                                if let Ok((members, state_number)) = committee {
                                                    let is_newer = {
                                                        let mut vm = validator_manager_b.write().await;
                                                        if state_number >= vm.state_number() {
                                                            vm.update_committee(&members, state_number);
                                                            info!("Updated committee with {} members for state {} (signed by {})", members.len(), state_number, envelope.node_id);
                                                            true
                                                        } else {
                                                            false
                                                        }
                                                    };
                                                    // Cache latest selection for serving/relaying
                                                    let newer_than_cache = committee_envelope_b.read().await
                                                        .as_ref()
                                                        .map(|(sn, _)| state_number > *sn)
                                                        .unwrap_or(true);
                                                    if newer_than_cache {
                                                        *committee_envelope_b.write().await =
                                                            Some((state_number, message.payload.clone()));
                                                        if is_newer {
                                                            if let Some(ref repo) = committee_repo_b {
                                                                let _ = repo.store(&members, state_number).await;
                                                            }
                                                            outbox.push((None, message.clone()));
                                                        }
                                                    }
                                                }
                                            }
                                        } else {
                                            info!("Rejected CommitteeSelection from {:?}: invalid signature, replay, or identity mismatch", addr);
                                        }
                                    } else {
                                        info!("Rejected CommitteeSelection from {:?}: malformed envelope (unsigned payloads are not accepted)", addr);
                                    }
                                }

                                // Handle SnapshotOffer -> pull the snapshot if newer
                                if message.message_type == MessageType::SnapshotOffer {
                                    info!("Processing SnapshotOffer from {:?}", addr);
                                    let metadata = Message::deserialize_snapshot_metadata(&message.payload);
                                    if let Ok((state_number, _, _, _, _, _, _, _, _, _, _)) = metadata {
                                        let current_state = {
                                            let sync = snapshot_sync_b.read().await;
                                            sync.get_current_state_number()
                                        };
                                        if state_number > current_state {
                                            info!("Received newer snapshot offer: state {} — requesting from {:?}", state_number, addr);
                                            let peer = PeerID::new(
                                                format!("peer-{}", addr), addr, addr.to_string());
                                            {
                                                let mut sync = snapshot_sync_b.write().await;
                                                let _ = sync.request_snapshot(peer, state_number);
                                            }
                                            if let Ok(req) = Message::serialize_snapshot_request(state_number) {
                                                outbox.push((Some(addr), Message::new(
                                                    MessageType::SnapshotRequest,
                                                    node_id_b.clone().into_bytes(),
                                                    req,
                                                )));
                                            }
                                        }
                                    }
                                }

                                // Handle SnapshotRequest -> serve stored snapshot data
                                if message.message_type == MessageType::SnapshotRequest {
                                    info!("Processing SnapshotRequest from {:?}", addr);
                                    if let Ok(state_number) = Message::deserialize_snapshot_request(&message.payload) {
                                        let snap = {
                                            let sync = snapshot_sync_b.read().await;
                                            sync.get_snapshot(state_number)
                                        };
                                        if let Some(snap) = snap {
                                            if let Ok(data) = Message::serialize_snapshot(&snap) {
                                                outbox.push((Some(addr), Message::new(
                                                    MessageType::SnapshotData,
                                                    node_id_b.clone().into_bytes(),
                                                    data,
                                                )));
                                            }
                                        } else {
                                            info!("SnapshotRequest for state {} — not stored locally", state_number);
                                        }
                                    }
                                }

                                // Handle SnapshotData -> verify, install, persist, re-offer
                                if message.message_type == MessageType::SnapshotData {
                                    info!("Processing SnapshotData from {:?}", addr);
                                    let snapshot = Message::deserialize_snapshot(&message.payload);
                                    if let Ok(snap) = snapshot {
                                        let installed = {
                                            let mut sync = snapshot_sync_b.write().await;
                                            sync.receive_snapshot(snap.clone()).is_ok()
                                        };
                                        if installed {
                                            info!("Received and stored snapshot: state {}", snap.state_number);
                                            if let Some(ref repo) = snapshot_repo_b {
                                                let _ = repo.store(&snap).await;
                                            }
                                            // Re-offer so peers that missed the original
                                            // advertisement can also pull it from us.
                                            if let Ok(meta) = Message::serialize_snapshot_metadata(&snap) {
                                                outbox.push((None, Message::new(
                                                    MessageType::SnapshotOffer,
                                                    node_id_b.clone().into_bytes(),
                                                    meta,
                                                )));
                                            }
                                        }
                                    }
                                }
                            }
                            Ok(Ok(None)) => {
                                // No message received (connection closed gracefully)
                            }
                            Ok(Err(e)) => {
                                info!("Receive error from {:?}: {:?}", addr, e);
                                connections_to_remove.push(addr);
                            }
                            Err(_) => {
                                // Timeout - normal
                            }
                        }
                    }

                    // Flush deferred sends (direct replies + relays)
                    for (target, msg) in outbox.drain(..) {
                        match target {
                            Some(t) => {
                                if let Some(c) = connections.get_mut(&t) {
                                    let _ = c.send(msg).await;
                                }
                            }
                            None => {
                                for (a, c) in connections.iter_mut() {
                                    if *a != addr {
                                        let _ = c.send(msg.clone()).await;
                                    }
                                }
                            }
                        }
                    }
                }
                
                // Remove failed connections
                if !connections_to_remove.is_empty() {
                    let mut connections = connections_b.write().await;
                    for addr in connections_to_remove {
                        info!("Removing failed connection {:?}", addr);
                        connections.remove(&addr);
                    }
                }
                
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
        });

        // Start heartbeat loop (configurable interval)
        tokio::spawn(async move {
            info!("Starting heartbeat loop ({}s interval)", heartbeat_interval_hb);
            let interval = tokio::time::Duration::from_secs(heartbeat_interval_hb);
            loop {
                let is_running = *running_hb.read().await;
                if !is_running {
                    break;
                }
                
                tokio::time::sleep(interval).await;
                
                // Register connected peers with tracker and send Ping to each
                let mut pings_to_send: Vec<(std::net::SocketAddr, u64)> = Vec::new();
                {
                    let connections = connections_hb.read().await;
                    let mut tracker = heartbeat_tracker_hb.write().await;
                    for addr in connections.keys() {
                        let peer_id = PeerID::new(
                            format!("peer-{}", addr),
                            *addr,
                            addr.to_string(),
                        );
                        tracker.get_or_create(peer_id.clone());
                    }
                    // Get peers due for ping
                    let due = tracker.peers_due_for_ping();
                    for (peer_id, ping_id) in due {
                        pings_to_send.push((peer_id.endpoint, ping_id));
                    }
                }
                
                // Send Ping messages to due peers
                if !pings_to_send.is_empty() {
                    let mut connections = connections_hb.write().await;
                    for (addr, ping_id) in pings_to_send {
                        if let Some(connection) = connections.get_mut(&addr) {
                            let ping = Message::new(
                                MessageType::Ping,
                                ping_id.to_be_bytes().to_vec(),
                                node_id_hb.clone().into_bytes(),
                            );
                            if connection.send(ping).await.is_ok() {
                                info!("Heartbeat: sent Ping to {:?} (id={})", addr, ping_id);
                            }
                        }
                    }
                }
                
                info!("Heartbeat: {} active connections", {
                    let connections = connections_hb.read().await;
                    connections.len()
                });
                
                // Check for missed heartbeats and mark unhealthy
                let mut unhealthy_peers = Vec::new();
                {
                    let mut tracker = heartbeat_tracker_hb.write().await;
                    unhealthy_peers = tracker.check_missed();
                }
                
                // Remove unhealthy peers from discovery and connections
                if !unhealthy_peers.is_empty() {
                    let mut discovery = discovery_hb.write().await;
                    let mut connections = connections_hb.write().await;
                    for peer_id in unhealthy_peers {
                        info!("Heartbeat: removing unhealthy peer {:?}", peer_id.endpoint);
                        let _ = discovery.remove_peer(&peer_id);
                        if let Some(addr) = connections.keys().find(|a| a.port() == peer_id.endpoint.port()).cloned() {
                            connections.remove(&addr);
                            info!("Heartbeat: removed connection to {:?}", addr);
                        }
                    }
                }
            }
        });

        // Start peer discovery loop (every 60s)
        tokio::spawn(async move {
            info!("Starting peer discovery loop (60s interval)");
            loop {
                let is_running = *running_pd.read().await;
                if !is_running {
                    break;
                }
                
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                
                let discovery = discovery_pd.read().await;
                let peer_count = discovery.get_peers().len();
                info!("Peer discovery: {} known peers", peer_count);
            }
        });

        // Start gossip maintenance loop (every 30s)
        tokio::spawn(async move {
            info!("Starting gossip maintenance loop (30s interval)");
            loop {
                let is_running = *running_gm.read().await;
                if !is_running {
                    break;
                }
                
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                
                let mut gossip = gossip_gm.write().await;
                gossip.cleanup_expired();
                info!("Gossip maintenance: cleaned up expired messages");
            }
        });

        // Start snapshot advertisement loop (every 24h - represented as 5min for testing)
        tokio::spawn(async move {
            info!("Starting snapshot advertisement loop (24h interval)");
            loop {
                let is_running = *running_sa.read().await;
                if !is_running {
                    break;
                }
                
                // Use 5 minutes for testing, would be 24h in production
                tokio::time::sleep(tokio::time::Duration::from_secs(300)).await;
                
                let snapshot_sync = snapshot_sync_sa.read().await;
                let available = snapshot_sync.get_available_snapshots();
                info!("Snapshot advertisement: {} available snapshots", available.len());
            }
        });

        // Autonomous announce task: periodically re-announce our own validator
        // (fresh envelope keeps relay caches within the freshness window),
        // gossip the PeerList, and re-serve cached state so late joiners and
        // peers that missed earlier broadcasts still converge.
        tokio::spawn(async move {
            loop {
                if !*running_an.read().await {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(announce_interval_an)).await;

                // 1) Re-announce own validator with a fresh envelope
                if let Some(ref validator) = self_validator_an {
                    if let Ok(inner) = Message::serialize_validator_registration(validator) {
                        let envelope = identity_an.sign_envelope("xnt-validator-registration", &inner);
                        if let Ok(payload) = Message::serialize_signed_envelope(&envelope) {
                            validator_envelopes_an.write().await
                                .insert(validator.address.clone(), payload.clone());
                            let msg = Message::new(
                                MessageType::ValidatorRegistration,
                                node_id_an.clone().into_bytes(),
                                payload,
                            );
                            let mut connections = connections_an.write().await;
                            for (_, c) in connections.iter_mut() {
                                let _ = c.send(msg.clone()).await;
                            }
                        }
                    }
                }

                // 2) Broadcast PeerList (verified announcements) so peers learn
                //    identities needed to authenticate relayed envelopes
                let announcements: Vec<SignedAnnouncement> = {
                    let known = known_announcements_an.read().await;
                    known.values().take(64).cloned().collect()
                };
                if let Ok(payload) = Message::serialize_announcements(&announcements) {
                    let msg = Message::new(
                        MessageType::PeerList,
                        node_id_an.clone().into_bytes(),
                        payload,
                    );
                    let mut connections = connections_an.write().await;
                    for (_, c) in connections.iter_mut() {
                        let _ = c.send(msg.clone()).await;
                    }
                }

                // 3) Re-serve cached validator envelopes + committee selection
                //    + latest snapshot offer (state sync beacon)
                let cached: Vec<Vec<u8>> = {
                    validator_envelopes_an.read().await.values().cloned().collect()
                };
                let committee_raw = committee_envelope_an.read().await.clone();
                let latest_snap = {
                    let sync = snapshot_sync_an.read().await;
                    sync.get_latest_local()
                };
                let mut connections = connections_an.write().await;
                for (_, c) in connections.iter_mut() {
                    for raw in &cached {
                        let _ = c.send(Message::new(
                            MessageType::ValidatorRegistration,
                            node_id_an.clone().into_bytes(),
                            raw.clone(),
                        )).await;
                    }
                    if let Some((_, ref raw)) = committee_raw {
                        let _ = c.send(Message::new(
                            MessageType::CommitteeSelection,
                            node_id_an.clone().into_bytes(),
                            raw.clone(),
                        )).await;
                    }
                    if let Some(ref snap) = latest_snap {
                        if let Ok(meta) = Message::serialize_snapshot_metadata(snap) {
                            let _ = c.send(Message::new(
                                MessageType::SnapshotOffer,
                                node_id_an.clone().into_bytes(),
                                meta,
                            )).await;
                        }
                    }
                }
            }
        });

        // Autonomous committee task: every epoch, deterministically derive the
        // committee from the active validator set (same input -> same output on
        // every node). The first member broadcasts the signed selection.
        tokio::spawn(async move {
            loop {
                if !*running_cm.read().await {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(committee_poll_cm)).await;
                if !committee_enabled_cm {
                    continue;
                }

                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() / committee_epoch_cm;

                let mut vm = validator_manager_cm.write().await;
                let active: Vec<String> = vm.get_active_validators()
                    .iter().map(|v| v.address.clone()).collect();
                let members = derive_committee(active, epoch);
                if members.is_empty() {
                    continue;
                }

                let changed = vm.get_current_committee()
                    .map(|c| c.state_number < epoch as i64 || c.members != members)
                    .unwrap_or(true);
                if !changed {
                    continue;
                }

                vm.update_committee(&members, epoch as i64);
                drop(vm);
                info!("Committee formed: {} members (epoch {}, leader {})",
                    members.len(), epoch, members[0]);

                if let Some(ref repo) = committee_repo_cm {
                    let _ = repo.store(&members, epoch as i64).await;
                }

                // Leader (first member post-rotation) broadcasts the signed
                // selection; other nodes already applied the same derivation.
                if members.first().map(|m| m.as_str()) == Some(own_validator_addr_cm.as_str()) {
                    if let Ok(inner) = Message::serialize_committee_selection(&members, epoch as i64) {
                        let envelope = identity_cm.sign_envelope("xnt-committee-selection", &inner);
                        if let Ok(payload) = Message::serialize_signed_envelope(&envelope) {
                            *committee_envelope_cm.write().await =
                                Some((epoch as i64, payload.clone()));
                            let msg = Message::new(
                                MessageType::CommitteeSelection,
                                node_id_cm.clone().into_bytes(),
                                payload,
                            );
                            let mut connections = connections_cm.write().await;
                            for (_, c) in connections.iter_mut() {
                                let _ = c.send(msg.clone()).await;
                            }
                        }
                    }
                }
            }
        });

        // Autonomous snapshot task: the committee leader produces one snapshot
        // per epoch, persists + serves it locally, and advertises it. Peers
        // pull the data via SnapshotRequest.
        tokio::spawn(async move {
            loop {
                if !*running_sp.read().await {
                    break;
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(snapshot_poll_sp)).await;
                if !snapshot_enabled_sp {
                    continue;
                }

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let snap_state = (now / snapshot_epoch_sp) as i64;

                // Producer = current committee leader
                let leader = {
                    let vm = validator_manager_sp.read().await;
                    vm.get_committee_members().first().cloned()
                };
                if leader.as_deref() != Some(own_validator_addr_sp.as_str()) {
                    continue;
                }

                let (produce, committee, active, pending) = {
                    let sync = snapshot_sync_sp.read().await;
                    let vm = validator_manager_sp.read().await;
                    (
                        snap_state > sync.get_current_state_number(),
                        vm.get_committee_members(),
                        vm.get_active_validators().iter().map(|v| v.address.clone()).collect::<Vec<_>>(),
                        vm.get_pending_validators().iter().map(|v| v.address.clone()).collect::<Vec<_>>(),
                    )
                };
                if !produce {
                    continue;
                }

                // Deterministic state hash over epoch + committee + validators
                let mut h: u64 = 0xcbf29ce484222325;
                let mut mix = |s: &str| {
                    for b in s.as_bytes() {
                        h ^= *b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                };
                mix(&snap_state.to_string());
                for m in &committee { mix(m); }
                for v in &active { mix(v); }
                let state_hash = format!("{:016x}", h);
                let ts = now as i64;
                let snapshot = xentrium_shared::Snapshot {
                    state_number: snap_state,
                    state_hash: state_hash.clone(),
                    snapshot_hash: xentrium_network::BasicSnapshotSync::snapshot_hash_for(
                        snap_state, &state_hash, ts),
                    timestamp: ts,
                    accounts: std::collections::HashMap::new(),
                    active_validators: active,
                    pending_validators: pending,
                    reward_pool_balance: 0,
                    last_reward_distribution: 0,
                    active_proposals: vec![],
                    proposal_votes: std::collections::HashMap::new(),
                    committee_signature: None,
                };

                {
                    let mut sync = snapshot_sync_sp.write().await;
                    let _ = sync.serve_snapshot(snapshot.clone());
                }
                if let Some(ref repo) = snapshot_repo_sp {
                    let _ = repo.store(&snapshot).await;
                }
                if let Ok(meta) = Message::serialize_snapshot_metadata(&snapshot) {
                    info!("Produced snapshot for state {} — advertising", snap_state);
                    let msg = Message::new(
                        MessageType::SnapshotOffer,
                        node_id_sp.clone().into_bytes(),
                        meta,
                    );
                    let mut connections = connections_sp.write().await;
                    for (_, c) in connections.iter_mut() {
                        let _ = c.send(msg.clone()).await;
                    }
                }
            }
        });

        info!("Background networking tasks started");
        Ok(())
    }

    /// Initialize gossip protocol
    async fn initialize_gossip(&self) -> Result<()> {
        info!("Initializing gossip protocol");
        // Gossip is already initialized in constructor
        info!("Gossip protocol initialized");
        Ok(())
    }

    /// Initialize snapshot sync
    async fn initialize_snapshot_sync(&self) -> Result<()> {
        info!("Initializing snapshot sync");
        // Snapshot sync is already initialized in constructor
        info!("Snapshot sync initialized");
        Ok(())
    }

    /// Initialize validator manager
    async fn initialize_validator_manager(&self) -> Result<()> {
        info!("Initializing validator manager");
        // Validator manager is already initialized in constructor
        info!("Validator manager initialized");
        Ok(())
    }
}

/// Load configuration from a TOML file
pub fn load_config(path: &str) -> Result<NodeConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| XentriumError::DatabaseError(format!("Failed to read config: {}", e)))?;
    
    let config: NodeConfig = toml::from_str(&content)
        .map_err(|e| XentriumError::DatabaseError(format!("Failed to parse config: {}", e)))?;
    
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NodeConfig::default();
        assert_eq!(config.node_id, "xentrium-node-1");
        assert_eq!(config.network_address, "127.0.0.1:30303");
        assert_eq!(config.max_peers, 50);
    }

    #[test]
    fn test_node_creation() {
        let config = NodeConfig::default();
        let node = Node::new(config).unwrap();
        assert_eq!(node.config().node_id, "xentrium-node-1");
    }

    #[tokio::test]
    async fn test_node_startup() {
        let mut config = NodeConfig::default();
        config.network_address = "127.0.0.1:39990".to_string();
        let node = Node::new(config).unwrap();
        
        node.start().await.unwrap();
        assert!(node.is_running().await);
        
        node.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_node_shutdown() {
        let mut config = NodeConfig::default();
        config.network_address = "127.0.0.1:39991".to_string();
        let node = Node::new(config).unwrap();
        
        node.start().await.unwrap();
        assert!(node.is_running().await);
        
        node.stop().await.unwrap();
        assert!(!node.is_running().await);
    }

    #[tokio::test]
    async fn test_discovery_initialization() {
        let mut config = NodeConfig::default();
        config.network_address = "127.0.0.1:39992".to_string();
        config.bootstrap_peers = vec!["127.0.0.1:30304".to_string()];
        
        let node = Node::new(config).unwrap();
        node.start().await.unwrap();
        
        // Discovery initialization completes without error
        // Bootstrap peers are added internally
        
        node.stop().await.unwrap();
    }

    #[tokio::test]
    async fn test_service_initialization() {
        let mut config = NodeConfig::default();
        config.network_address = "127.0.0.1:39993".to_string();
        let node = Node::new(config).unwrap();
        
        node.start().await.unwrap();
        
        // Verify all services are initialized (Arc types are always valid)
        // Just verify they exist by calling methods on them
        let _ = node.validator_manager();
        let _ = node.discovery();
        let _ = node.gossip();
        let _ = node.snapshot_sync();
        
        node.stop().await.unwrap();
    }
}
