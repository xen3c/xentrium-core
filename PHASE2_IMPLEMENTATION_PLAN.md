# Xentrium Phase 2 Implementation Plan

**Date:** 2026-11-02
**Protocol Version:** TAD v2.2.7 (Frozen)
**Current Status:** Phase 1 Complete (16/16 tests passing)

---

## Executive Summary

**Objective:** Extend Phase 1 single-node implementation to multi-node network with P2P consensus.

**Scope:**
- Create validator_manager module
- Create xentrium-network crate
- Create xentrium-node crate
- Implement integration tests

**Estimated Duration:** 10 weeks

---

## Component Overview

| Component | Status | Complexity | Dependencies |
|-----------|--------|------------|--------------|
| validator_manager | ❌ Missing | Medium | xentrium-shared, xentrium-storage |
| xentrium-network | ❌ Missing | High | xentrium-shared, xentrium-crypto, quinn |
| xentrium-node | ❌ Missing | Medium | All Phase 1 + network |
| Integration Tests | ❌ Missing | High | All Phase 2 components |

---

## 1. validator_manager Implementation Plan

### 1.1 File Structure

```
xentrium-consensus/src/
├── lib.rs                          # Export validator_manager
├── state.rs                        # Existing
├── transaction_processor.rs        # Existing
└── validator_manager.rs            # NEW FILE
```

### 1.2 Responsibilities

**Primary Responsibilities:**
1. Maintain active and pending validator lists
2. Select committee members (dynamic sizing)
3. Rotate committee on schedule
4. Collect and aggregate committee votes
5. Track validator uptime and reputation
6. Manage validator state transitions

**Secondary Responsibilities:**
1. Validate validator eligibility
2. Calculate validator weights
3. Enforce committee weight cap (5%)
4. Generate committee signatures

### 1.3 Interfaces

**Public API:**
```rust
pub struct ValidatorManager {
    // State
    active_validators: HashMap<Address, ValidatorInfo>,
    pending_validators: HashMap<Address, ValidatorInfo>,
    current_committee: CommitteeInfo,
    vote_counter: VoteCounter,
    
    // Dependencies
    pool: SqlitePool,
}

impl ValidatorManager {
    // Lifecycle
    pub fn new(pool: SqlitePool) -> Self;
    pub async fn load_validators(&self) -> Result<()>;
    
    // Validator Management
    pub async fn register_validator(&self, data: ValidatorRegistrationData) -> Result<()>;
    pub async fn enable_validator(&self, address: Address) -> Result<()>;
    pub async fn disable_validator(&self, address: Address) -> Result<()>;
    pub async fn update_uptime(&self, address: Address, score: i32) -> Result<()>;
    
    // Committee Selection
    pub fn select_committee(&self) -> Result<CommitteeInfo>;
    pub fn get_current_committee(&self) -> &CommitteeInfo;
    pub fn should_rotate_committee(&self) -> bool;
    
    // Voting
    pub fn start_voting(&mut self, proposal: GovernanceProposal);
    pub fn add_vote(&mut self, vote: CommitteeVote) -> Result<()>;
    pub fn get_vote_result(&self) -> GovernanceStatus;
    pub fn is_committee_member(&self, address: &Address) -> bool;
}
```

**Data Structures:**
```rust
pub struct CommitteeInfo {
    pub members: Vec<Address>,
    pub selection_time: i64,
    pub state_number: i64,
    pub threshold: f64,  // 0.67
}

pub struct VoteCounter {
    proposal_id: u64,
    approve_votes: u64,
    reject_votes: u64,
    votes: HashMap<Address, bool>,
    committee_weights: HashMap<Address, f64>,
}
```

### 1.4 Dependencies

**Internal:**
- xentrium-shared: types, constants, errors
- xentrium-storage: ValidatorRepository, AccountRepository

**External:**
- sqlx: Database queries
- rand: Random selection
- chrono: Timestamps

### 1.5 Implementation Details

**Committee Selection Algorithm:**
```
1. Filter active validators (uptime_score >= 80)
2. Sort by reputation_score (descending)
3. Take top min(500, active_validators)
4. Calculate weight = sqrt(reputation_score)
5. Apply weight cap: max(weight, 5% of total)
6. Weighted random selection without replacement
7. Select min(51, active_validators) members
8. Store committee with timestamp
```

**Vote Aggregation:**
```
1. Only committee members can vote
2. Each vote has weight equal to validator_weight
3. Accumulate approve_weight and reject_weight
4. Calculate approve_ratio = approve_weight / total_weight
5. If approve_ratio >= 0.67: Approved
6. If reject_ratio >= 0.33: Rejected
7. Otherwise: Voting (continue)
```

### 1.6 Test Plan

**Unit Tests:**
- test_committee_selection_bootstrap (0-20 validators)
- test_committee_selection_full (21+ validators)
- test_committee_size_capping (51+ validators)
- test_pool_size_capping (500+ validators)
- test_weight_cap_enforcement
- test_vote_aggregation_approval
- test_vote_aggregation_rejection
- test_vote_aggregation_continuing
- test_validator_registration
- test_validator_state_transitions

---

## 2. xentrium-network Implementation Plan

### 2.1 Crate Structure

```
xentrium-network/
├── Cargo.toml
└── src/
    ├── lib.rs                      # Public API exports
    ├── peer.rs                     # Peer identity and state
    ├── discovery.rs                # Peer discovery mechanism
    ├── protocol.rs                 # Message definitions
    ├── transport.rs                # QUIC transport layer
    ├── gossip.rs                   # Gossip protocol
    └── snapshot_sync.rs            # Snapshot distribution
```

### 2.2 Responsibilities

**peer.rs:**
- PeerID structure (address, public_key, endpoint, machine_id)
- Peer state management (Connecting, Connected, Disconnected, Banned)
- Peer validation (signature verification, machine_id check)
- Peer reputation tracking

**discovery.rs:**
- Static bootstrap peer connection
- DNS seed resolution
- P2P peer exchange (gossip)
- Peer list maintenance
- Connection attempt scheduling

**protocol.rs:**
- Protocol Buffers message definitions
- Message serialization/deserialization
- Message type routing
- Message validation

**transport.rs:**
- QUIC endpoint initialization
- Outbound connection establishment
- Inbound connection acceptance
- Connection lifecycle management
- Keep-alive heartbeats
- Reconnection logic

**gossip.rs:**
- Transaction propagation
- State update propagation
- Committee announcement propagation
- Vote propagation
- Message de-duplication
- TTL management

**snapshot_sync.rs:**
- Snapshot discovery
- Snapshot offer/request exchange
- Snapshot data streaming
- Snapshot hash verification
- Transaction replay after snapshot

### 2.3 Interfaces

**Public API:**
```rust
pub struct NetworkManager {
    transport: QuicTransport,
    peers: Arc<RwLock<HashMap<PeerID, Peer>>>,
    gossip: GossipProtocol,
    snapshot_sync: SnapshotSync,
}

impl NetworkManager {
    // Lifecycle
    pub fn new(config: NetworkConfig) -> Result<Self>;
    pub async fn start(&self) -> Result<()>;
    pub async fn shutdown(&self) -> Result<()>;
    
    // Peer Management
    pub async fn connect_to_peer(&self, endpoint: SocketAddr) -> Result<()>;
    pub async fn disconnect_peer(&self, peer_id: PeerID) -> Result<()>;
    pub fn get_peers(&self) -> Vec<PeerID>;
    
    // Messaging
    pub async fn broadcast_transaction(&self, tx: Transaction) -> Result<()>;
    pub async fn broadcast_state_update(&self, state: NetworkState) -> Result<()>;
    pub async fn send_vote(&self, vote: CommitteeVote, target: PeerID) -> Result<()>;
    
    // Snapshot Sync
    pub async fn request_snapshot(&self, state_number: i64) -> Result<Snapshot>;
    pub async fn serve_snapshot(&self, snapshot: Snapshot) -> Result<()>;
    
    // Callbacks
    pub fn on_transaction(&self, callback: fn(Transaction));
    pub fn on_state_update(&self, callback: fn(NetworkState));
    pub fn on_vote(&self, callback: fn(CommitteeVote));
}
```

### 2.4 Dependencies

**Internal:**
- xentrium-shared: types, constants, errors
- xentrium-crypto: KeyPair, signing, verification

**External:**
- quinn: QUIC transport
- rustls: TLS for QUIC
- prost: Protocol Buffers
- tokio: Async runtime
- serde: Serialization
- tracing: Logging

### 2.5 Implementation Details

**QUIC Transport:**
```
1. Create Quinn endpoint with TLS config
2. Load self-signed certificate from wallet
3. Bind to configured address:port
4. Start incoming connection listener
5. For outbound: dial peer endpoint
6. Perform handshake with PeerID exchange
7. Verify peer signature
8. Establish bidirectional streams
9. Start heartbeat timer (30s)
10. On timeout: mark peer disconnected
```

**Gossip Protocol:**
```
1. Maintain message cache (transaction ID, state hash)
2. On new transaction: broadcast to all connected peers
3. On new state update: broadcast to all connected peers
4. On receive message: check cache, if seen -> drop
5. Add to cache with TTL (1h for tx, 24h for state)
6. Process message
7. If gossipable: rebroadcast to other peers
```

**Snapshot Sync:**
```
1. Node A needs snapshot
2. A sends SnapshotRequest(state_number) to peers
3. Peers respond with SnapshotOffer(hash, size)
4. A selects peer with valid hash
5. A requests snapshot data
6. Peer streams snapshot via QUIC
7. A receives chunks, verifies hash on completion
8. A applies snapshot to local state
9. A requests transactions after snapshot
10. A replay transactions to reach current state
```

### 2.6 Test Plan

**Unit Tests:**
- test_peer_id_creation
- test_peer_validation
- test_handshake_protocol
- test_message_serialization
- test_message_deserialization
- test_gossip_deduplication
- test_gossip_propagation
- test_snapshot_request
- test_snapshot_streaming
- test_connection_establishment
- test_reconnection_logic

**Integration Tests:**
- test_peer_discovery (with mock peers)
- test_transaction_gossip (2 peers)
- test_state_update_gossip (2 peers)
- test_snapshot_sync (2 peers)
- test_connection_lifecycle

---

## 3. xentrium-node Implementation Plan

### 3.1 Crate Structure

```
xentrium-node/
├── Cargo.toml
└── src/
    ├── main.rs              # CLI entry point
    ├── node.rs              # Node orchestration
    ├── config.rs            # Configuration management
    ├── cli.rs               # Command-line interface
    └── api.rs               # REST/gRPC API (optional, Phase 3)
```

### 3.2 Responsibilities

**main.rs:**
- CLI argument parsing
- Command routing (start, stop, wallet, etc.)
- Error handling and logging setup

**node.rs:**
- Node lifecycle management (startup, operational, shutdown)
- Component initialization order
- Dependency injection
- Signal handling (SIGINT, SIGTERM)
- Graceful shutdown coordination

**config.rs:**
- Configuration file parsing (TOML)
- Configuration validation
- Default values
- Configuration serialization

**cli.rs:**
- Wallet commands (create, unlock, lock, address, balance)
- Node commands (start, stop, status, peers)
- Validator commands (register, enable, disable)
- Governance commands (proposal, vote)
- Snapshot commands (create, sync, list)

### 3.3 Interfaces

**Public API:**
```rust
pub struct XentriumNode {
    config: NodeConfig,
    wallet: Wallet,
    database: SqlitePool,
    state_engine: StateEngine,
    validator_manager: ValidatorManager,
    network: NetworkManager,
    shutdown_signal: Arc<AtomicBool>,
}

impl XentriumNode {
    // Lifecycle
    pub fn new(config: NodeConfig) -> Result<Self>;
    pub async fn start(&mut self) -> Result<()>;
    pub async fn shutdown(&mut self) -> Result<()>;
    
    // Configuration
    pub fn load_config(path: &str) -> Result<NodeConfig>;
    
    // Wallet
    pub fn get_wallet(&self) -> &Wallet;
    
    // State
    pub fn get_state(&self) -> &NetworkState;
    
    // Network
    pub fn get_network(&self) -> &NetworkManager;
    
    // CLI Commands
    pub async fn cmd_start(&mut self) -> Result<()>;
    pub async fn cmd_stop(&mut self) -> Result<()>;
    pub async fn cmd_status(&self) -> Result<NodeStatus>;
}
```

**Configuration Structure:**
```rust
pub struct NodeConfig {
    pub node: NodeSection,
    pub network: NetworkSection,
    pub consensus: ConsensusSection,
    pub api: ApiSection,
    pub logging: LoggingSection,
}

pub struct NodeSection {
    pub address: String,
    pub port: u16,
    pub data_dir: String,
}

pub struct NetworkSection {
    pub bootstrap_peers: Vec<String>,
    pub max_peers: usize,
    pub heartbeat_interval_secs: u64,
    pub connection_timeout_secs: u64,
}
```

### 3.4 Dependencies

**Internal:**
- xentrium-shared: types, constants, errors
- xentrium-crypto: Wallet, KeyPair
- xentrium-storage: Database, repositories
- xentrium-consensus: StateEngine, ValidatorManager, TransactionProcessor
- xentrium-network: NetworkManager

**External:**
- tokio: Async runtime
- toml: Configuration parsing
- clap: CLI argument parsing
- tracing: Logging
- signal-hook: Signal handling

### 3.5 Implementation Details

**Startup Sequence:**
```
1. Parse CLI arguments
2. Load configuration from config.toml
3. Initialize logging
4. Create data directory if missing
5. Initialize SQLite database
6. Load or create wallet
7. Unlock wallet if password provided
8. Load local state from database
9. Initialize StateEngine
10. Initialize ValidatorManager
11. Initialize NetworkManager
12. Connect to bootstrap peers
13. Perform peer discovery
14. Request state synchronization
15. If snapshot needed: download and apply
16. Start gossip loop
17. Start heartbeat sender
18. Start transaction processor
19. Start snapshot scheduler
20. Start committee rotation scheduler
21. Start API server (if enabled)
22. Register signal handlers
23. Enter operational loop
```

**Operational Loop:**
```
while !shutdown_signal.load() {
    // Periodic tasks
    check_auto_lock();
    process_pending_transactions();
    update_validator_uptime();
    check_committee_rotation();
    check_snapshot_time();
    
    // Network events (via callbacks)
    // - On transaction received
    // - On state update received
    // - On vote received
    
    sleep(100ms);
}
```

**Graceful Shutdown:**
```
1. Set shutdown signal
2. Stop accepting new transactions
3. Process remaining pending transactions
4. Create final snapshot
5. Persist state to database
6. Close all peer connections
7. Stop network manager
8. Close database
9. Lock wallet
10. Exit
```

### 3.6 Test Plan

**Unit Tests:**
- test_config_parsing
- test_config_validation
- test_node_initialization
- test_startup_sequence
- test_shutdown_sequence
- test_signal_handling

**Integration Tests:**
- test_node_with_mock_network
- test_node_startup_with_existing_database
- test_node_startup_with_empty_database
- test_cli_wallet_commands
- test_cli_node_commands

---

## 4. Integration Test Plan

### 4.1 Test Infrastructure

**Test Framework:**
- Use existing test infrastructure
- Create `xentrium/tests/integration/` directory
- Use `tokio::test` for async tests
- Use temporary directories for test data

**Test Utilities:**
```rust
// tests/common/mod.rs
pub struct TestNode {
    node: XentriumNode,
    temp_dir: TempDir,
}

impl TestNode {
    pub async fn new(config: NodeConfig) -> Self;
    pub async fn start(&mut self) -> Result<()>;
    pub async fn stop(&mut self) -> Result<()>;
    pub fn endpoint(&self) -> SocketAddr;
}
```

### 4.2 Test Scenarios

**Test 1: 2-Node Communication**
```
Setup:
- Create Node A on port 30303
- Create Node B on port 30304
- Configure A as bootstrap peer for B

Execute:
- Start Node A
- Start Node B
- Wait for B to connect to A (10s timeout)
- Verify peer count on both nodes (A: 1, B: 1)
- Send transaction from A
- Verify B receives transaction via gossip
- Stop both nodes

Assertions:
- A.peers.len() == 1
- B.peers.len() == 1
- B.received_transactions.contains(tx_id)
```

**Test 2: Snapshot Synchronization**
```
Setup:
- Create Node A with existing state (state_number = 100)
- Create Node B with empty state (state_number = 0)
- Configure A as bootstrap peer for B

Execute:
- Start Node A
- Start Node B
- Wait for B to request snapshot (10s timeout)
- Wait for snapshot download (30s timeout)
- Verify B state_number == 100
- Verify B state_hash matches A
- Stop both nodes

Assertions:
- B.state_number == 100
- B.state_hash == A.state_hash
- B.accounts == A.accounts
```

**Test 3: Validator Communication**
```
Setup:
- Create 3 validator nodes (A, B, C)
- All nodes have validator wallets
- Enable all validators

Execute:
- Start all 3 nodes
- Wait for committee selection (simulate time passage)
- Verify committee formation (3 nodes, < 21 threshold = bootstrap)
- Simulate governance proposal
- Collect votes from all validators
- Verify vote aggregation
- Stop all nodes

Assertions:
- All nodes in validator list
- Committee formed (or bootstrap mode)
- Votes collected successfully
- Vote result calculated correctly
```

**Test 4: Committee Vote Propagation**
```
Setup:
- Create 5 validator nodes (simulate 21+ active)
- All nodes in full consensus mode
- Wait for committee selection (5 nodes -> committee size 5)

Execute:
- Create governance proposal on Node A
- Broadcast proposal via gossip
- Wait for all nodes to receive proposal
- Each node casts vote
- Wait for vote propagation (10s timeout)
- Verify all nodes collected all votes
- Verify vote result consistency across nodes
- Stop all nodes

Assertions:
- All nodes received proposal
- All nodes cast votes
- All nodes collected all votes
- Vote result identical on all nodes
```

### 4.3 Test Implementation

**File Structure:**
```
xentrium/tests/
├── common/
│   └── mod.rs              # Test utilities
├── integration/
│   ├── mod.rs
│   ├── two_node_communication.rs
│   ├── snapshot_sync.rs
│   ├── validator_communication.rs
│   └── committee_voting.rs
└── lib.rs
```

**Test Configuration:**
```toml
[[test]]
name = "integration"
path = "tests/integration"
harness = false
```

---

## 5. Build Order

### Phase 2.1: Foundation (Week 1-2)

**Step 1: validator_manager Module**
- File: `xentrium-consensus/src/validator_manager.rs`
- Dependencies: xentrium-shared, xentrium-storage
- Estimated effort: 3 days
- Risk: Low (well-defined algorithm from TAD)

**Step 2: xentrium-network Crate Structure**
- Files: Cargo.toml, lib.rs, module stubs
- Dependencies: xentrium-shared, xentrium-crypto
- Estimated effort: 1 day
- Risk: Low (structural work only)

**Step 3: Protocol Buffers Definitions**
- File: `xentrium-network/src/protocol.rs`
- Define all message types
- Generate Rust code with prost
- Estimated effort: 2 days
- Risk: Low (message format specified in TAD)

### Phase 2.2: Networking Core (Week 3-4)

**Step 4: QUIC Transport Implementation**
- File: `xentrium-network/src/transport.rs`
- Implement Quinn endpoint, connections, heartbeats
- Estimated effort: 4 days
- Risk: Medium (QUIC complexity, TLS configuration)

**Step 5: Peer Identity and Discovery**
- Files: `xentrium-network/src/peer.rs`, `discovery.rs`
- Implement PeerID, validation, bootstrap logic
- Estimated effort: 3 days
- Risk: Medium (peer verification logic)

**Step 6: Gossip Protocol**
- File: `xentrium-network/src/gossip.rs`
- Implement message propagation, de-duplication
- Estimated effort: 3 days
- Risk: Medium (message ordering, cache management)

### Phase 2.3: Sync and Consensus (Week 5-6)

**Step 7: Snapshot Synchronization**
- File: `xentrium-network/src/snapshot_sync.rs`
- Implement snapshot offer/request/download
- Estimated effort: 4 days
- Risk: High (streaming, verification, large data)

**Step 8: Committee Vote Propagation**
- Extend gossip.rs for vote messages
- Implement vote collection callback
- Estimated effort: 2 days
- Risk: Low (extension of gossip)

**Step 9: validator_manager Integration**
- Integrate validator_manager with state engine
- Implement committee rotation trigger
- Estimated effort: 2 days
- Risk: Low (integration work)

### Phase 2.4: Node Orchestration (Week 7-8)

**Step 10: xentrium-node Crate Structure**
- Files: Cargo.toml, lib.rs, module stubs
- Estimated effort: 1 day
- Risk: Low (structural work)

**Step 11: Configuration Management**
- File: `xentrium-node/src/config.rs`
- Implement TOML parsing, validation
- Estimated effort: 2 days
- Risk: Low (well-defined format)

**Step 12: Node Lifecycle**
- File: `xentrium-node/src/node.rs`
- Implement startup sequence, shutdown, signal handling
- Estimated effort: 4 days
- Risk: Medium (complex coordination)

**Step 13: CLI Interface**
- File: `xentrium-node/src/cli.rs`
- Implement wallet, node, validator commands
- Estimated effort: 3 days
- Risk: Low (straightforward CLI)

### Phase 2.5: Integration (Week 9-10)

**Step 14: Network Integration Tests**
- File: `xentrium/tests/integration/two_node_communication.rs`
- Implement 2-node test
- Estimated effort: 2 days
- Risk: Medium (requires working network)

**Step 15: Snapshot Sync Tests**
- File: `xentrium/tests/integration/snapshot_sync.rs`
- Implement snapshot sync test
- Estimated effort: 2 days
- Risk: High (requires working sync)

**Step 16: Validator Communication Tests**
- File: `xentrium/tests/integration/validator_communication.rs`
- Implement validator test
- Estimated effort: 2 days
- Risk: Medium (requires validator_manager)

**Step 17: Committee Voting Tests**
- File: `xentrium/tests/integration/committee_voting.rs`
- Implement voting test
- Estimated effort: 2 days
- Risk: High (requires full stack)

**Step 18: Bug Fixes and Polish**
- Fix integration test failures
- Add error handling edge cases
- Performance optimization
- Estimated effort: 3 days
- Risk: Variable (depends on bugs found)

---

## 6. Risk Analysis

### 6.1 validator_manager

**Complexity:** Medium
**Dependencies:** xentrium-shared, xentrium-storage
**Estimated Difficulty:** 3/10

**Risks:**
- Committee selection algorithm correctness
- Weight calculation precision
- Vote aggregation edge cases
- Race conditions in vote collection

**Mitigations:**
- Comprehensive unit tests
- Mathematical verification of selection algorithm
- Mutex protection for shared state
- Extensive integration testing

---

### 6.2 xentrium-network

**Complexity:** High
**Dependencies:** xentrium-shared, xentrium-crypto, quinn, rustls, prost
**Estimated Difficulty:** 8/10

**Risks:**
- QUIC configuration complexity
- TLS certificate management
- NAT traversal issues
- Network partition handling
- Message ordering guarantees
- Memory leaks in connection management
- Performance under load

**Mitigations:**
- Start with simple connection tests
- Use self-signed certificates (Phase 2)
- Implement connection pooling
- Add extensive logging
- Memory profiling during development
- Load testing before integration

---

### 6.3 xentrium-node

**Complexity:** Medium
**Dependencies:** All Phase 1 + xentrium-network
**Estimated Difficulty:** 5/10

**Risks:**
- Startup sequence complexity
- Dependency initialization order
- Graceful shutdown reliability
- Signal handling cross-platform
- Configuration validation edge cases

**Mitigations:**
- Document clear startup phases
- Use dependency injection pattern
- Test shutdown repeatedly
- Use signal-hook crate for portability
- Extensive config validation tests

---

### 6.4 Integration Tests

**Complexity:** High
**Dependencies:** All Phase 2 components
**Estimated Difficulty:** 7/10

**Risks:**
- Flaky tests due to timing
- Resource cleanup issues
- Test isolation failures
- Debugging distributed issues
- Mock infrastructure complexity

**Mitigations:**
- Use explicit timeouts
- Ensure proper cleanup in teardown
- Use isolated temp directories
- Add detailed logging in tests
- Start with unit tests before integration

---

## 7. Recommended Starting Component

### Primary Recommendation: validator_manager

**Reasoning:**
1. **Low Complexity:** Well-defined algorithm from TAD v2.2.7
2. **No External Dependencies:** Only uses xentrium-shared and xentrium-storage
3. **Self-Contained:** Can be tested independently without network
4. **Foundation for Network:** Committee selection needed before P2P voting
5. **Low Risk:** Algorithm is deterministic and verifiable
6. **Quick Win:** Can be completed in 3-5 days
7. **Validates Design:** Proves committee selection logic before network complexity

**Alternative: xentrium-network (if validator_manager blocked)**
- Higher risk but can be started in parallel
- Requires external dependencies (quinn, rustls, prost)
- More complex but independent of validator_manager

### Implementation Strategy

**Recommended Sequence:**
1. Implement validator_manager (Week 1)
2. Implement xentrium-network structure (Week 1)
3. Implement QUIC transport (Week 2-3)
4. Implement gossip protocol (Week 3-4)
5. Implement snapshot sync (Week 4-5)
6. Implement xentrium-node (Week 6-7)
7. Implement integration tests (Week 8-10)

**Parallel Development:**
- validator_manager and network structure can be done in parallel
- Unit tests for each component can be written during implementation
- Integration tests only after all components complete

---

## 8. Success Criteria

Phase 2 is complete when:

**validator_manager:**
- ✅ Committee selection algorithm implemented correctly
- ✅ Dynamic sizing (min(51, active)) works
- ✅ Vote aggregation passes all unit tests
- ✅ Validator state transitions work

**xentrium-network:**
- ✅ QUIC connections establish between 2+ nodes
- ✅ Gossip propagates messages to all nodes
- ✅ Snapshot sync completes successfully
- ✅ Peer discovery works

**xentrium-node:**
- ✅ Node starts up completely
- ✅ Node shuts down gracefully
- ✅ CLI commands work
- ✅ Configuration loads correctly

**Integration Tests:**
- ✅ 2-node communication test passes
- ✅ Snapshot sync test passes
- ✅ Validator communication test passes
- ✅ Committee voting test passes

**Overall:**
- ✅ Multi-node network runs
- ✅ Transactions propagate across network
- ✅ State synchronizes across nodes
- ✅ Committee voting works
- ✅ No critical bugs or crashes

---

## 9. Dependencies Graph

```
xentrium-node
├── xentrium-network
│   ├── xentrium-shared
│   └── xentrium-crypto
├── xentrium-consensus
│   ├── xentrium-shared
│   ├── xentrium-crypto
│   ├── xentrium-storage
│   │   ├── xentrium-shared
│   │   └── sqlx
│   └── validator_manager (NEW)
│       ├── xentrium-shared
│       └── xentrium-storage
└── xentrium-crypto
    └── xentrium-shared
```

**Build Order:**
1. xentrium-shared (✅ Complete)
2. xentrium-crypto (✅ Complete)
3. xentrium-storage (✅ Complete)
4. xentrium-consensus (✅ Core complete, needs validator_manager)
5. validator_manager (❌ Missing - START HERE)
6. xentrium-network (❌ Missing - after validator_manager)
7. xentrium-node (❌ Missing - last)

---

## 10. Resource Requirements

### Development Resources

**Developer Time:**
- 1 senior developer (full-time)
- Estimated: 10 weeks

**Hardware for Testing:**
- 3 nodes or VMs for multi-node testing
- Minimum: 2 CPU cores, 4GB RAM per node
- Network: Local network or LAN

**External Services:**
- None required (P2P design)
- Optional: DNS seed resolver for bootstrap

### Build Requirements

**Rust Toolchain:**
- rustc 1.70+
- cargo 1.70+

**System Libraries:**
- OpenSSL (for rustls TLS)
- SQLite3 (already included via sqlx)

---

## 11. Open Questions and Decisions Needed

### 11.1 Protocol Questions

**Q1: MachineID Verification**
- How to verify MachineID without centralized service?
- **Decision:** Self-reported with reputation-based trust (as designed in PHASE2_DESIGN.md)

**Q2: Committee Signature Aggregation**
- How to efficiently aggregate 67% of signatures?
- **Decision:** Collect all signatures, verify >= 67% threshold during vote aggregation

**Q3: Snapshot Compression**
- Should snapshots be compressed during transfer?
- **Decision:** Phase 2: No compression (streaming is sufficient), Phase 3: Add zstd compression

### 11.2 Implementation Questions

**Q4: Protocol Buffers vs Serde**
- Should we use Protobufs for all messages or just network?
- **Decision:** Protobufs for network messages (cross-language compatibility), Serde for storage

**Q5: Message Ordering**
- Do we need ordered message delivery?
- **Decision:** No, gossip is unordered, use timestamps for ordering

**Q6: Connection Pooling**
- Should we limit concurrent connections per peer?
- **Decision:** Yes, max 1 connection per peer, max 100 total peers

### 11.3 Testing Questions

**Q7: Test Isolation**
- How to ensure integration tests don't interfere?
- **Decision:** Use unique ports and temp directories for each test

**Q8: Test Data**
- Should we generate synthetic test data or use real protocol data?
- **Decision:** Generate synthetic data for determinism, validate against real protocol

---

## 12. Milestones

### Milestone 1: validator_manager Complete (Week 1)
- validator_manager.rs implemented
- All unit tests passing
- Integrated with xentrium-consensus

### Milestone 2: Network Foundation (Week 2)
- xentrium-network crate created
- Protocol Buffers defined
- Basic peer structure implemented

### Milestone 3: QUIC Transport (Week 3)
- QUIC connections work
- Handshake protocol works
- Peer identity verification works

### Milestone 4: Gossip Protocol (Week 4)
- Transaction propagation works
- Message de-duplication works
- 2-node communication test passes

### Milestone 5: Snapshot Sync (Week 5)
- Snapshot download works
- Snapshot verification works
- Snapshot sync test passes

### Milestone 6: Node Orchestration (Week 7)
- xentrium-node crate created
- Node startup works
- Node shutdown works
- CLI commands work

### Milestone 7: Full Integration (Week 9)
- All integration tests passing
- Multi-node network runs
- Committee voting works

### Milestone 8: Phase 2 Complete (Week 10)
- All components integrated
- Bug fixes complete
- Documentation updated
- Ready for Phase 3

---

## 13. Contingency Plans

### If validator_manager Blocked

**Alternative 1:** Simplify Committee Selection
- Implement static committee (first 51 validators)
- Remove weighted random selection
- Add dynamic selection in Phase 3

**Alternative 2:** Skip Committee, Implement Gossip First
- Focus on xentrium-network gossip
- Implement manual committee later
- Use Bootstrap mode until committee ready

### If xentrium-network Blocked

**Alternative 1:** Use HTTP Instead of QUIC
- Implement simple HTTP-based messaging
- QUIC can be added in Phase 3
- Performance acceptable for Phase 2

**Alternative 2:** Use Existing P2P Library
- Use libp2p-rust instead of quinn
- Higher-level abstraction
- More dependencies but less complexity

### If Integration Tests Blocked

**Alternative 1:** Focus on Unit Tests
- Increase unit test coverage to 90%
- Add mock-based integration tests
- Real network tests in Phase 3

**Alternative 2:** Manual Testing
- Document manual test procedures
- Developer manually runs multi-node setup
- Automated tests added later

---

## 14. Documentation Requirements

### Code Documentation

**Requirements:**
- All public structs must have doc comments
- All public functions must have doc comments
- Complex algorithms must have inline comments
- Protocol interactions must be documented

### User Documentation

**Required Documents:**
- README.md (overview, quick start)
- INSTALL.md (build instructions, dependencies)
- CONFIG.md (configuration reference)
- CLI.md (command reference)
- NETWORK.md (network architecture, peer discovery)
- OPERATOR_GUIDE.md (node operation, troubleshooting)

### Developer Documentation

**Required Documents:**
- ARCHITECTURE.md (update with Phase 2 components)
- API.md (public API reference)
- TESTING.md (test strategy, how to run tests)
- CONTRIBUTING.md (contribution guidelines)

---

## 15. Post-Phase 2 Deliverables

### Code Deliverables

1. ✅ validator_manager.rs module
2. ✅ xentrium-network crate (7 modules)
3. ✅ xentrium-node crate (4 modules)
4. ✅ Integration test suite (4 tests)

### Documentation Deliverables

1. ✅ PHASE2_DESIGN.md (already created)
2. ✅ PHASE2_IMPLEMENTATION_PLAN.md (this document)
3. ✅ Updated ARCHITECTURE.md
4. ✅ README.md (updated for multi-node)
5. ✅ CONFIG.md (configuration reference)
6. ✅ CLI.md (command reference)

### Test Deliverables

1. ✅ validator_manager unit tests (10 tests)
2. ✅ xentrium-network unit tests (10 tests)
3. ✅ xentrium-node unit tests (5 tests)
4. ✅ Integration tests (4 tests)
5. ✅ Test report (results, coverage)

---

## Conclusion

This implementation plan provides a complete roadmap for Phase 2 multi-node implementation:

**Key Recommendations:**
1. **Start with validator_manager** (low complexity, high value)
2. **Build xentrium-network foundation** before complex features
3. **Integrate components incrementally** with testing at each step
4. **Focus on integration tests** to validate multi-node behavior
5. **Document extensively** for future maintainability

**Estimated Timeline:** 10 weeks
**Risk Level:** Medium (network complexity, integration testing)
**Success Probability:** High (clear requirements, solid Phase 1 foundation)

**Next Step:** Begin Step 1 - Implement validator_manager module.

---

**Document Status:** READY FOR IMPLEMENTATION
