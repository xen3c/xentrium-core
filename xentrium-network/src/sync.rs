use crate::{PeerID, Result, NetworkError};
use xentrium_shared::Snapshot;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Snapshot metadata for advertisement
#[derive(Debug, Clone)]
pub struct SnapshotMetadata {
    pub state_number: i64,
    pub state_hash: String,
    pub snapshot_hash: String,
    pub timestamp: i64,
    pub size_bytes: usize,
    pub peer_id: PeerID,
}

/// Snapshot synchronization trait
pub trait SnapshotSync {
    fn request_snapshot(&mut self, peer_id: PeerID, state_number: i64) -> Result<()>;
    fn serve_snapshot(&mut self, snapshot: Snapshot) -> Result<()>;
    fn receive_snapshot(&mut self, snapshot: Snapshot) -> Result<()>;
    fn verify_snapshot(&self, snapshot: &Snapshot) -> Result<bool>;
    fn advertise_snapshot(&mut self, metadata: SnapshotMetadata) -> Result<()>;
    fn get_available_snapshots(&self) -> Vec<SnapshotMetadata>;
    fn get_current_state_number(&self) -> i64;
    fn set_current_state_number(&mut self, state_number: i64);
}

/// Basic snapshot sync implementation
pub struct BasicSnapshotSync {
    current_state_number: i64,
    local_snapshots: HashMap<i64, Snapshot>,
    advertised_snapshots: HashMap<PeerID, Vec<SnapshotMetadata>>,
    pending_requests: HashMap<i64, Instant>,
    snapshot_timeout: Duration,
}

impl BasicSnapshotSync {
    pub fn new(state_number: i64) -> Self {
        Self {
            current_state_number: state_number,
            local_snapshots: HashMap::new(),
            advertised_snapshots: HashMap::new(),
            pending_requests: HashMap::new(),
            snapshot_timeout: Duration::from_secs(300), // 5 minutes
        }
    }

    pub fn with_timeout(state_number: i64, timeout_secs: u64) -> Self {
        Self {
            current_state_number: state_number,
            local_snapshots: HashMap::new(),
            advertised_snapshots: HashMap::new(),
            pending_requests: HashMap::new(),
            snapshot_timeout: Duration::from_secs(timeout_secs),
        }
    }

    fn cleanup_expired_requests(&mut self) {
        let now = Instant::now();
        let mut expired = Vec::new();
        
        for (state_number, requested_at) in &self.pending_requests {
            if now.duration_since(*requested_at) > self.snapshot_timeout {
                expired.push(*state_number);
            }
        }
        
        for state_number in expired {
            self.pending_requests.remove(&state_number);
        }
    }

    fn compute_snapshot_hash(snapshot: &Snapshot) -> String {
        // Simple hash computation for verification
        format!("{}:{}:{}", snapshot.state_number, snapshot.state_hash, snapshot.timestamp)
    }
}

impl SnapshotSync for BasicSnapshotSync {
    fn request_snapshot(&mut self, peer_id: PeerID, state_number: i64) -> Result<()> {
        // Check if requesting a newer snapshot
        if state_number <= self.current_state_number {
            return Err(NetworkError::ValidationError(
                "Cannot request older or equal snapshot".to_string(),
            ));
        }

        // Check for duplicate requests
        if self.pending_requests.contains_key(&state_number) {
            return Err(NetworkError::ValidationError(
                "Snapshot request already pending".to_string(),
            ));
        }

        // Track the request
        self.pending_requests.insert(state_number, Instant::now());
        
        // In a real implementation, this would send a request message to the peer
        Ok(())
    }

    fn serve_snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        // Store the snapshot locally
        self.local_snapshots.insert(snapshot.state_number, snapshot.clone());
        
        // Update current state number if this is newer
        if snapshot.state_number > self.current_state_number {
            self.current_state_number = snapshot.state_number;
        }
        
        Ok(())
    }

    fn receive_snapshot(&mut self, snapshot: Snapshot) -> Result<()> {
        // Verify the snapshot first
        if !self.verify_snapshot(&snapshot)? {
            return Err(NetworkError::ValidationError(
                "Snapshot verification failed".to_string(),
            ));
        }

        // Check if this is a newer snapshot
        if snapshot.state_number <= self.current_state_number {
            return Err(NetworkError::ValidationError(
                "Received snapshot is not newer".to_string(),
            ));
        }

        // Store the snapshot
        self.local_snapshots.insert(snapshot.state_number, snapshot.clone());
        
        // Update current state number
        self.current_state_number = snapshot.state_number;
        
        // Remove from pending requests
        self.pending_requests.remove(&snapshot.state_number);
        
        Ok(())
    }

    fn verify_snapshot(&self, snapshot: &Snapshot) -> Result<bool> {
        // Verify snapshot hash matches computed hash
        let computed_hash = Self::compute_snapshot_hash(snapshot);
        
        // Check if snapshot_hash matches computed hash
        let is_valid = snapshot.snapshot_hash == computed_hash;
        
        // Additional validation: check timestamp is reasonable
        let is_timestamp_valid = snapshot.timestamp > 0;
        
        Ok(is_valid && is_timestamp_valid)
    }

    fn advertise_snapshot(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        // Add to advertised snapshots for this peer
        self.advertised_snapshots
            .entry(metadata.peer_id.clone())
            .or_insert_with(Vec::new)
            .push(metadata);
        
        Ok(())
    }

    fn get_available_snapshots(&self) -> Vec<SnapshotMetadata> {
        let mut all_metadata = Vec::new();
        
        // Add local snapshots
        for (_, snapshot) in &self.local_snapshots {
            let metadata = SnapshotMetadata {
                state_number: snapshot.state_number,
                state_hash: snapshot.state_hash.clone(),
                snapshot_hash: snapshot.snapshot_hash.clone(),
                timestamp: snapshot.timestamp,
                size_bytes: std::mem::size_of_val(snapshot),
                peer_id: PeerID::new(
                    "local".to_string(),
                    "127.0.0.1:0".parse().unwrap(),
                    "local".to_string(),
                ),
            };
            all_metadata.push(metadata);
        }
        
        // Add advertised snapshots from peers
        for (_, metadata_list) in &self.advertised_snapshots {
            all_metadata.extend(metadata_list.clone());
        }
        
        // Sort by state number (newest first)
        all_metadata.sort_by(|a, b| b.state_number.cmp(&a.state_number));
        
        all_metadata
    }

    fn get_current_state_number(&self) -> i64 {
        self.current_state_number
    }

    fn set_current_state_number(&mut self, state_number: i64) {
        self.current_state_number = state_number;
    }
}

impl Default for BasicSnapshotSync {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_snapshot(state_number: i64) -> Snapshot {
        let state_hash = format!("state_hash_{}", state_number);
        let timestamp = state_number * 1000;
        let snapshot_hash = format!("{}:{}:{}", state_number, state_hash, timestamp);
        
        Snapshot {
            state_number,
            state_hash,
            snapshot_hash,
            timestamp,
            accounts: HashMap::new(),
            active_validators: vec![],
            pending_validators: vec![],
            reward_pool_balance: 0,
            last_reward_distribution: 0,
            active_proposals: vec![],
            proposal_votes: HashMap::new(),
            committee_signature: None,
        }
    }

    #[test]
    fn test_basic_snapshot_sync_creation() {
        let sync = BasicSnapshotSync::new(0);
        assert_eq!(sync.get_current_state_number(), 0);
    }

    #[test]
    fn test_request_snapshot() {
        let mut sync = BasicSnapshotSync::new(0);
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        let result = sync.request_snapshot(peer_id, 100);
        assert!(result.is_ok());
    }

    #[test]
    fn test_request_snapshot_invalid() {
        let mut sync = BasicSnapshotSync::new(100);
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        let result = sync.request_snapshot(peer_id, 50);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_request_handling() {
        let mut sync = BasicSnapshotSync::new(0);
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        
        // First request
        sync.request_snapshot(peer_id.clone(), 100).unwrap();
        
        // Duplicate request
        let result = sync.request_snapshot(peer_id, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_serve_snapshot() {
        let mut sync = BasicSnapshotSync::new(0);
        let snapshot = create_test_snapshot(100);
        let result = sync.serve_snapshot(snapshot);
        assert!(result.is_ok());
        assert_eq!(sync.get_current_state_number(), 100);
    }

    #[test]
    fn test_receive_snapshot() {
        let mut sync = BasicSnapshotSync::new(0);
        let snapshot = create_test_snapshot(100);
        let result = sync.receive_snapshot(snapshot);
        assert!(result.is_ok());
        assert_eq!(sync.get_current_state_number(), 100);
    }

    #[test]
    fn test_receive_snapshot_invalid() {
        let mut sync = BasicSnapshotSync::new(100);
        let snapshot = create_test_snapshot(50);
        let result = sync.receive_snapshot(snapshot);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_snapshot() {
        let sync = BasicSnapshotSync::new(0);
        let snapshot = create_test_snapshot(100);
        let result = sync.verify_snapshot(&snapshot);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_advertise_snapshot() {
        let mut sync = BasicSnapshotSync::new(0);
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        
        let metadata = SnapshotMetadata {
            state_number: 100,
            state_hash: "state_hash".to_string(),
            snapshot_hash: "snap_hash".to_string(),
            timestamp: 0,
            size_bytes: 1024,
            peer_id: peer_id.clone(),
        };
        
        let result = sync.advertise_snapshot(metadata);
        assert!(result.is_ok());
        
        let available = sync.get_available_snapshots();
        assert_eq!(available.len(), 1);
    }

    #[test]
    fn test_get_available_snapshots() {
        let mut sync = BasicSnapshotSync::new(0);
        
        // Add local snapshot
        let snapshot = create_test_snapshot(100);
        sync.serve_snapshot(snapshot).unwrap();
        
        let available = sync.get_available_snapshots();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].state_number, 100);
    }

    #[test]
    fn test_newer_snapshot_selection() {
        let mut sync = BasicSnapshotSync::new(0);
        
        // Add multiple snapshots
        sync.serve_snapshot(create_test_snapshot(50)).unwrap();
        sync.serve_snapshot(create_test_snapshot(100)).unwrap();
        sync.serve_snapshot(create_test_snapshot(75)).unwrap();
        
        let available = sync.get_available_snapshots();
        assert_eq!(available.len(), 3);
        // Should be sorted newest first
        assert_eq!(available[0].state_number, 100);
        assert_eq!(available[1].state_number, 75);
        assert_eq!(available[2].state_number, 50);
    }
}
