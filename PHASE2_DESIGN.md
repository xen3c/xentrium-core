# Xentrium Phase 2 Design Document

**Date:** 2026-11-02
**Protocol Version:** TAD v2.2.7 (Frozen)
**Scope:** Multi-node network implementation

---

## Overview

Phase 2 extends the Phase 1 single-node implementation to support:
- P2P networking with QUIC
- Peer discovery and identity
- Multi-node consensus with committee voting
- Snapshot synchronization
- Validator management and committee selection

---

## 1. xentrium-network Crate Design

### 1.1 Crate Structure

```
xentrium-network/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── peer.rs           // Peer identity and state
    ├── discovery.rs      // Peer discovery mechanism
    ├── protocol.rs       // Message formats and serialization
    ├── transport.rs      // QUIC transport layer
    ├── gossip.rs         // Gossip protocol for propagation
    └── snapshot_sync.rs  // Snapshot distribution
```

### 1.2 Dependencies

```toml
[dependencies]
xentrium-shared = { path = "../xentrium-shared" }
xentrium-crypto = { path = "../xentrium-crypto" }
quinn = "0.11"              # QUIC implementation
rustls = "0.23"             # TLS for QUIC
prost = "0.12"              # Protocol Buffers
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = "0.1"             # Logging
```

### 1.3 Peer Identity

**PeerID Structure:**
```rust
pub struct PeerID {
    pub address: String,           // XNT address (base58 public key)
    public_key: VerifyingKey,      // Ed25519 public key
    endpoint: SocketAddr,         // Network endpoint (IP:port)
    machine_id: String,           // MachineID (for validator identity)
    last_seen: Instant,           // Last contact timestamp
    state: PeerState,             // Current peer state
}

pub enum PeerState {
    Connecting,
    Connected,
    Disconnected,
    Banned,
}
```

**Peer Discovery Strategy:**
- Static bootstrap peers (config file)
- DNS seed resolution
- P2P gossip for peer exchange
- Validator peer discovery via MachineID

### 1.4 Message Formats

**Message Types (Protocol Buffers):**

```protobuf
message XentriumMessage {
    MessageType type = 1;
    uint64 timestamp = 2;
    bytes sender = 3;              // PeerID bytes
    oneof payload {
        Handshake handshake = 10;
        TransactionMessage transaction = 11;
        StateUpdate state_update = 12;
        SnapshotOffer snapshot_offer = 13;
        SnapshotRequest snapshot_request = 14;
        SnapshotData snapshot_data = 15;
        CommitteeVote committee_vote = 16;
        Heartbeat heartbeat = 17;
        PeerList peer_list = 18;
    }
}

message Handshake {
    string address = 1;
    bytes public_key = 2;
    string machine_id = 3;
    uint64 state_number = 4;
    string state_hash = 5;
}

message TransactionMessage {
    bytes transaction = 1;         // Serialized Transaction
}

message StateUpdate {
    uint64 state_number = 1;
    string state_hash = 2;
    repeated bytes transactions = 3;
}

message SnapshotOffer {
    uint64 state_number = 1;
    string snapshot_hash = 2;
    uint64 size = 3;
}

message SnapshotRequest {
    uint64 state_number = 1;
}

message SnapshotData {
    Snapshot snapshot = 1;
}

message CommitteeVote {
    uint64 proposal_id = 1;
    bool vote = 2;
    bytes signature = 3;
}

message Heartbeat {
    uint64 sequence = 1;
}

message PeerList {
    repeated PeerInfo peers = 1;
}

message PeerInfo {
    string address = 1;
    string endpoint = 2;
    string machine_id = 3;
}
```

### 1.5 Transport Layer (QUIC)

**Connection Architecture:**
```rust
pub struct QuicTransport {
    endpoint: quinn::Endpoint,
    incoming: quinn::Incoming,
    connections: Arc<RwLock<HashMap<PeerID, Connection>>>,
}

pub struct Connection {
    peer_id: PeerID,
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}
```

**Connection Lifecycle:**
1. **Outbound:** Dial peer endpoint, perform handshake, establish connection
2. **Inbound:** Accept incoming connection, verify handshake, add to peer table
3. **Maintenance:** Heartbeat every 30s, disconnect on timeout (120s)
4. **Reconnection:** Exponential backoff (1s, 2s, 4s, 8s, max 60s)

### 1.6 Gossip Protocol

**Gossip Strategy:**
- **Push:** Push new transactions to connected peers
- **Pull:** Request peer list and state updates periodically
- **Push-Pull:** For critical state changes (committee formation, snapshot finality)

**Gossip Message Flow:**
```
Node A (new transaction) → Gossip to peers → Peers validate → Gossip to their peers
```

**De-duplication:**
- Transaction ID cache (last 1000 seen transactions)
- State number + hash cache (last 100 seen states)
- TTL: 1 hour for transaction cache, 24 hours for state cache

### 1.7 Snapshot Distribution

**Snapshot Synchronization:**

**Sync Trigger:**
- Node starts (bootstrap from zero)
- State number gap detected (> 10 blocks behind)
- Manual sync request via CLI

**Sync Process:**
1. **Discovery:** Request snapshot list from peers
2. **Selection:** Choose most recent snapshot with valid hash
3. **Download:** Stream snapshot data via QUIC
4. **Verification:** Validate snapshot hash and committee signature
5. **Replay:** Apply transactions after snapshot
6. **Finalize:** Update local state

**Snapshot Message Flow:**
```
Node A → SnapshotRequest(state_number) → Node B
Node B → SnapshotOffer(hash, size) → Node A
Node A → Accept → Node B
Node B → SnapshotData (streamed) → Node A
Node A → Verify hash → Apply → Complete
```

---

## 2. validator_manager Module Design

### 2.1 Module Structure

```
xentrium-consensus/src/
├── lib.rs
├── state.rs
├── transaction_processor.rs
└── validator_manager.rs       # NEW MODULE
```

### 2.2 Core Structures

```rust
pub struct ValidatorManager {
    active_validators: HashMap<Address, ValidatorInfo>,
    pending_validators: HashMap<Address, ValidatorInfo>,
    candidate_pool: Vec<Address>,
    current_committee: Vec<Address>,
    committee_selection_time: Option<i64>,
}

pub struct CommitteeInfo {
    pub members: Vec<Address>,
    pub selection_time: i64,
    pub state_number: i64,
    pub threshold: f64,  // 0.67
}
```

### 2.3 Committee Selection Algorithm

**Dynamic Committee Sizing (TAD v2.2.7):**
```
committee_size = min(51, active_validators)
candidate_pool_size = min(500, active_validators)
```

**Selection Process:**

1. **Build Candidate Pool:**
   - All active validators with uptime_score >= 80
   - Top candidate_pool_size by reputation_score
   - Exclude validators in Suspended state

2. **Weighted Random Selection:**
   - Calculate selection weight: sqrt(reputation_score)
   - Apply weight cap: max(weight, 5% of total weight)
   - Weighted random draw without replacement
   - Select committee_size validators

3. **Committee Formation:**
   - Assign committee membership
   - Record selection timestamp
   - Store committee signature (once 67% approve)

**Pseudocode:**
```rust
fn select_committee(active_validators: Vec<ValidatorInfo>) -> Vec<Address> {
    let committee_size = min(51, active_validators.len());
    let pool_size = min(500, active_validators.len());
    
    // Filter candidates
    let candidates: Vec<_> = active_validators
        .into_iter()
        .filter(|v| v.uptime_score >= 80)
        .take(pool_size)
        .collect();
    
    // Calculate weights
    let mut weights: Vec<f64> = candidates
        .iter()
        .map(|v| v.reputation_score.sqrt())
        .collect();
    
    // Apply weight cap (5%)
    let total_weight: f64 = weights.iter().sum();
    let max_weight = total_weight * 0.05;
    for w in weights.iter_mut() {
        *w = (*w).min(max_weight);
    }
    
    // Weighted random selection
    let committee = weighted_random_select(&candidates, &weights, committee_size);
    
    committee
}
```

### 2.4 Committee Rotation

**Rotation Schedule:**
- Committee rotates every 24 hours (at snapshot time)
- Snapshot finality triggers new committee selection
- Validators can serve on consecutive committees (no cooldown)

**Rotation Process:**
1. Snapshot finality achieved
2. Calculate new committee from active validators
3. Broadcast new committee via gossip
4. Validators confirm committee membership
5. Committee becomes active for next epoch

### 2.5 Validator Lifecycle Management

**Validator States:**
```
Pending (7 days) → Active → Suspended → (reactivation possible)
                ↓
            (slashing)
```

**State Transitions:**
- **EnableValidator tx:** Pending → Active (after 7 days)
- **DisableValidator tx:** Active → Suspended
- **Slashing:** Active → Suspended (permanent)
- **Reactivation:** Suspended → Active (requires new EnableValidator tx)

**Uptime Tracking:**
- Track consecutive uptime in weeks
- Update every heartbeat
- Reset on disconnection > 1 hour
- Committee eligibility requires 80% uptime score

---

## 3. Node Startup Lifecycle

### 3.1 Node Architecture

```
xentrium-node/
├── Cargo.toml
└── src/
    ├── main.rs              // CLI entry point
    ├── node.rs              // Node orchestration
    ├── config.rs            // Configuration management
    ├── cli.rs               // Command-line interface
    └── api.rs               // REST/gRPC API (optional)
```

### 3.2 Startup Sequence

**Phase 1: Initialization**
```
1. Load configuration (config.toml)
2. Initialize logging
3. Load or create wallet
4. Initialize SQLite database
5. Load local state from database
```

**Phase 2: Network Bootstrap**
```
6. Initialize QUIC transport
7. Connect to bootstrap peers
8. Perform peer discovery
9. Exchange handshakes with peers
10. Validate peer identities
```

**Phase 3: State Synchronization**
```
11. Request latest state number from peers
12. If local state is outdated (> 10 blocks):
    a. Request snapshot list
    b. Download latest snapshot
    c. Verify snapshot hash
    d. Apply snapshot
    e. Request transactions after snapshot
13. Otherwise: request pending transactions
```

**Phase 4: Consensus Activation**
```
14. Determine consensus phase (bootstrap vs full)
15. If bootstrap (0-20 validators):
    a. Operate in simple validation mode
    b. No committee voting required
16. If full consensus (21+ validators):
    a. Join candidate pool if validator
    b. Wait for committee selection
    c. Participate in committee voting if selected
```

**Phase 5: Operational Loop**
```
17. Start gossip message loop
18. Start heartbeat sender (30s interval)
19. Start transaction processor
20. Start snapshot scheduler (24h interval)
21. Start committee rotation scheduler (24h interval)
22. Start API server (if enabled)
```

### 3.3 Graceful Shutdown

**Shutdown Sequence:**
```
1. Stop accepting new transactions
2. Process pending transactions
3. Create final snapshot
4. Persist state to database
5. Close all peer connections
6. Close database
7. Exit
```

**Signal Handling:**
- SIGINT/SIGTERM: Graceful shutdown
- SIGUSR1: Force snapshot
- SIGUSR2: Dump debug state

### 3.4 Configuration Structure

**config.toml:**
```toml
[node]
address = "0.0.0.0"
port = 30303
data_dir = "./data"

[network]
bootstrap_peers = ["192.168.1.100:30303", "seed.xentrium.io:30303"]
max_peers = 100
heartbeat_interval_secs = 30
connection_timeout_secs = 120

[consensus]
auto_committee_rotation = true
snapshot_interval_hours = 24

[api]
enabled = true
bind_address = "127.0.0.1"
port = 8080

[logging]
level = "info"
file = "./xentrium.log"
```

---

## 4. Committee Vote Propagation

### 4.1 Vote Message Format

```protobuf
message CommitteeVote {
    uint64 proposal_id = 1;
    bool vote = 2;              // true = approve, false = reject
    bytes signature = 3;        // Validator signature
    uint64 timestamp = 4;
}
```

### 4.2 Vote Collection Process

**Vote Lifecycle:**
```
1. Proposal created (GovernanceProposal)
2. Voting period starts (30 days)
3. Validators receive proposal via gossip
4. Validators sign votes with private keys
5. Votes broadcast via gossip
6. All validators collect votes
7. After voting period: tally votes
8. If approve >= 67%: Approved
9. If reject >= 33%: Rejected
10. Result broadcast via gossip
```

### 4.3 Vote Aggregation

**Vote Counter:**
```rust
pub struct VoteCounter {
    proposal_id: u64,
    approve_votes: u64,
    reject_votes: u64,
    votes: HashMap<Address, bool>,  // voter -> vote
    committee_weights: HashMap<Address, f64>,
}

impl VoteCounter {
    pub fn add_vote(&mut self, voter: Address, vote: bool, weight: f64) {
        self.votes.insert(voter, vote);
        if vote {
            self.approve_votes += weight as u64;
        } else {
            self.reject_votes += weight as u64;
        }
    }
    
    pub fn result(&self) -> GovernanceStatus {
        let total = self.approve_votes + self.reject_votes;
        if total == 0 {
            return GovernanceStatus::Proposed;
        }
        
        let approve_ratio = self.approve_votes as f64 / total as f64;
        
        if approve_ratio >= 0.67 {
            GovernanceStatus::Approved
        } else if (1.0 - approve_ratio) >= 0.33 {
            GovernanceStatus::Rejected
        } else {
            GovernanceStatus::Voting
        }
    }
}
```

### 4.4 Vote Propagation Timing

**Propagation Requirements:**
- Vote broadcast immediately after signing
- Gossip propagation delay: < 5 seconds to 95% of committee
- Vote deduplication by (proposal_id, voter_address)
- Vote TTL: 35 days (voting period + 5 days buffer)

---

## 5. Validator Communications

### 5.1 Validator-to-Validator Messages

**Message Types:**
1. **Heartbeat:** Keep-alive, state number, uptime info
2. **State Hash Broadcast:** New state hash after block processing
3. **Committee Announcement:** New committee composition
4. **Vote Request:** Request vote on proposal
5. **Vote Response:** Submit vote
6. **Slashing Report:** Report misbehaving validator

### 5.2 Communication Channels

**Unicast (P2P):**
- Direct QUIC connection between peers
- Used for: snapshot transfer, private queries, large data

**Multicast (Gossip):**
- Broadcast to all connected peers
- Used for: transactions, votes, state updates, committee announcements

### 5.3 Validator Committee Protocol

**Committee Phase Timeline:**
```
T0: Snapshot finality achieved
T0+1s: New committee selection calculated
T0+2s: Committee announcement broadcast
T0+10s: All validators acknowledge committee
T0+24h: Next snapshot, committee rotates
```

**Committee Operations:**
1. **State Finalization:** Committee members sign state hash
2. **Governance Voting:** Committee members vote on proposals
3. **Slashing Decisions:** Committee votes on slashing proposals
4. **Emergency Stops:** Committee can halt network (2/3 supermajority)

---

## 6. Implementation Phases

### Phase 2.1: Foundation (Week 1-2)
- ✅ Create xentrium-network crate structure
- ✅ Implement QUIC transport layer
- ✅ Implement peer identity and discovery
- ✅ Define Protocol Buffers messages
- ✅ Implement handshake protocol

### Phase 2.2: Gossip and Sync (Week 3-4)
- ✅ Implement gossip protocol
- ✅ Implement transaction propagation
- ✅ Implement snapshot synchronization
- ✅ Implement state update propagation
- ✅ Add message de-duplication

### Phase 2.3: Consensus (Week 5-6)
- ✅ Implement validator_manager module
- ✅ Implement committee selection algorithm
- ✅ Implement committee rotation
- ✅ Implement vote collection and aggregation
- ✅ Implement vote propagation

### Phase 2.4: Node Orchestration (Week 7-8)
- ✅ Create xentrium-node crate
- ✅ Implement node startup lifecycle
- ✅ Implement configuration management
- ✅ Implement CLI interface
- ✅ Implement graceful shutdown

### Phase 2.5: Integration (Week 9-10)
- ✅ Integrate network with consensus
- ✅ Integrate validator_manager with state engine
- ✅ End-to-end multi-node testing
- ✅ Snapshot sync testing
- ✅ Committee voting testing

---

## 7. Security Considerations

### 7.1 Network Security

**QUIC + TLS:**
- Mandatory TLS for all QUIC connections
- Certificate-based peer authentication
- Ed25519 public key embedded in certificates
- Peer identity verification before connection

**Mitigations:**
- Reject unauthenticated connections
- Rate limit connection attempts
- Ban peers sending invalid messages
- Encrypt all sensitive data in transit

### 7.2 Validator Security

**Sybil Resistance:**
- MachineID verification (bootstrap exception limited)
- Reputation-based committee weight
- Committee weight cap (5%)
- Economic barrier (validator minimum balance)

**Slashing Protection:**
- Committee can vote to slash malicious validators
- Slashing removes validator from active set
- Slashed validators cannot rejoin without new registration

### 7.3 Snapshot Security

**Snapshot Validation:**
- Verify snapshot hash against state
- Verify committee signature (67% threshold)
- Reject snapshots without valid signatures
- Reject snapshots from unknown committee members

---

## 8. Performance Targets

### 8.1 Network Performance

- **Connection establishment:** < 5 seconds
- **Message propagation:** < 5 seconds to 95% of network
- **Snapshot download:** 1 GB in < 60 seconds
- **Peer discovery:** < 30 seconds to find 10 peers

### 8.2 Consensus Performance

- **Committee selection:** < 1 second
- **Vote aggregation:** < 5 seconds after voting period
- **State hash calculation:** < 100ms per state
- **Snapshot creation:** < 10 seconds

### 8.3 Resource Limits

- **Max peers:** 100 concurrent connections
- **Message queue:** 10,000 pending messages
- **Transaction pool:** 5,000 pending transactions
- **Peer cache:** 1,000 known peers

---

## 9. Testing Strategy

### 9.1 Unit Tests

- **Network:** QUIC connection, message serialization, peer discovery
- **Validator Manager:** Committee selection, rotation, vote aggregation
- **Node:** Startup sequence, configuration loading, shutdown

### 9.2 Integration Tests

- **Multi-node simulation:** 5 nodes in single process
- **Gossip propagation:** Measure message spread time
- **Snapshot sync:** Download and verify snapshot
- **Committee voting:** Simulate proposal and vote collection

### 9.3 Network Tests

- **P2P network:** Real multi-node deployment (3+ nodes)
- **Latency measurement:** End-to-end transaction processing
- **Partition tolerance:** Network partition and recovery
- **Bootstrap test:** New node joining existing network

---

## 10. Dependencies on Phase 1

### Required from Phase 1

✅ **xentrium-shared:** Constants, types, errors
✅ **xentrium-crypto:** KeyPair, wallet, signing
✅ **xentrium-storage:** Database, repositories
✅ **xentrium-consensus:** State engine, transaction processor

### New Dependencies

- **quinn:** QUIC transport
- **rustls:** TLS for QUIC
- **prost:** Protocol Buffers
- **tracing:** Structured logging

---

## 11. Open Questions

### 11.1 Protocol Questions

1. **Bootstrap Exception:** How to verify MachineID without centralized service?
   - *Design decision:* Self-reported MachineID with reputation-based trust

2. **Committee Signature:** How to aggregate 67% of signatures efficiently?
   - *Design decision:* Collect all committee signatures, verify >= 67% threshold

3. **Snapshot Size:** What is expected snapshot size in production?
   - *Design decision:* Assume 1-10 GB, stream via QUIC

### 11.2 Implementation Questions

1. **Protocol Buffers vs Serde:** Should we use Protobufs for all messages?
   - *Design decision:* Protobufs for network messages, Serde for storage

2. **Message Ordering:** Do we need ordered message delivery?
   - *Design decision:* No, gossip is unordered, use timestamps

3. **Peer Reputation:** How to track peer reputation across restarts?
   - *Design decision:* Store in database, load on startup

---

## 12. Success Criteria

Phase 2 is complete when:

1. ✅ xentrium-network crate implements all message types
2. ✅ QUIC transport connects 3+ nodes
3. ✅ Gossip propagates transactions to all nodes
4. ✅ Snapshot sync works between nodes
5. ✅ validator_manager selects committees correctly
6. ✅ Committee votes propagate and aggregate
7. ✅ Node startup completes without errors
8. ✅ Graceful shutdown persists state
9. ✅ Multi-node integration tests pass
10. ✅ Network partition recovery works

---

## Conclusion

This design document provides the architecture for Phase 2 multi-node implementation:

- **xentrium-network:** QUIC-based P2P networking with gossip
- **validator_manager:** Committee selection and vote aggregation
- **xentrium-node:** Node orchestration and lifecycle management

All designs follow TAD v2.2.7 specifications and maintain protocol compliance.

**Next Step:** Begin Phase 2.1 implementation (network foundation).
