pub mod config;

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use xentrium_consensus::ValidatorManager;
use xentrium_network::{BasicDiscovery, BasicGossip, BasicSnapshotSync, Discovery, TcpTransport, TcpConnection, PeerID, Peer, PeerState, Message, MessageType, NetworkConnection, SnapshotSync, HeartbeatTracker, NodeIdentity, SignedAnnouncement, ReplayGuard, TlsCredentials, TlsTransportConfig, tls_dir};
use xentrium_storage::{Database, ValidatorRepository, SnapshotRepository, CommitteeRepository, PeerRepository};
use xentrium_shared::{Result, XentriumError, ValidatorInfo, Snapshot};
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
}

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
        }
    }
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

        info!("Node started successfully: {}", self.config.node_id);
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
            if let Ok(Some(validator)) = validator_repo.get(&"test_validator".to_string()).await {
                let mut vm = self.validator_manager.write().await;
                let _ = vm.register_validator_from_network(validator);
                info!("Loaded validator from storage");
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
                                            // Cache verified announcements for relay
                                            let mut known = known_announcements_b.write().await;
                                            for ann in valid_anns {
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
                                        // authenticated handshake identity
                                        let auth_match = connection.auth_node_id.as_deref()
                                            == Some(envelope.node_id.as_str())
                                            && connection.auth_pubkey.as_deref()
                                                == Some(envelope.public_key.as_slice());
                                        if fresh && auth_match {
                                            if let Ok(inner) = verified {
                                                let validator = Message::deserialize_validator_registration(&inner);
                                                if let Ok(validator_info) = validator {
                                                    let mut vm = validator_manager_b.write().await;
                                                    let _ = vm.register_validator_from_network(validator_info.clone());
                                                    info!("Registered validator: {} (signed by {})", validator_info.address, envelope.node_id);
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
                                        let auth_match = connection.auth_node_id.as_deref()
                                            == Some(envelope.node_id.as_str())
                                            && connection.auth_pubkey.as_deref()
                                                == Some(envelope.public_key.as_slice());
                                        if fresh && auth_match {
                                            if let Ok(inner) = verified {
                                                let committee = Message::deserialize_committee_selection(&inner);
                                                if let Ok((members, state_number)) = committee {
                                                    let mut vm = validator_manager_b.write().await;
                                                    vm.update_committee(&members, state_number);
                                                    info!("Updated committee with {} members for state {} (signed by {})", members.len(), state_number, envelope.node_id);
                                                }
                                            }
                                        } else {
                                            info!("Rejected CommitteeSelection from {:?}: invalid signature, replay, or identity mismatch", addr);
                                        }
                                    } else {
                                        info!("Rejected CommitteeSelection from {:?}: malformed envelope (unsigned payloads are not accepted)", addr);
                                    }
                                }

                                // Handle SnapshotOffer
                                if message.message_type == MessageType::SnapshotOffer {
                                    info!("Processing SnapshotOffer from {:?}", addr);
                                    let metadata = Message::deserialize_snapshot_metadata(&message.payload);
                                    if let Ok((state_number, _, _, _, _, _, _, _, _, _, _)) = metadata {
                                        let sync = snapshot_sync_b.read().await;
                                        let current_state = sync.get_current_state_number();
                                        if state_number > current_state {
                                            info!("Received newer snapshot offer: state {}", state_number);
                                        }
                                    }
                                }

                                // Handle SnapshotRequest
                                if message.message_type == MessageType::SnapshotRequest {
                                    info!("Processing SnapshotRequest from {:?}", addr);
                                }

                                // Handle SnapshotData
                                if message.message_type == MessageType::SnapshotData {
                                    info!("Processing SnapshotData from {:?}", addr);
                                    let snapshot = Message::deserialize_snapshot(&message.payload);
                                    if let Ok(snap) = snapshot {
                                        let mut sync = snapshot_sync_b.write().await;
                                        let _ = sync.receive_snapshot(snap);
                                        info!("Received and stored snapshot");
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
