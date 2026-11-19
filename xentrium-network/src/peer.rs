use std::net::SocketAddr;
use std::time::Instant;
use xentrium_shared::Address;
use crate::Result;

/// Peer identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerID {
    pub address: Address,
    pub endpoint: SocketAddr,
    pub machine_id: String,
}

impl PeerID {
    pub fn new(address: Address, endpoint: SocketAddr, machine_id: String) -> Self {
        Self {
            address,
            endpoint,
            machine_id,
        }
    }
}

/// Peer state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerState {
    Connecting,
    Connected,
    Disconnected,
    Banned,
}

/// Peer information
#[derive(Debug, Clone)]
pub struct Peer {
    pub id: PeerID,
    pub state: PeerState,
    pub last_seen: Instant,
    pub uptime_seconds: u64,
    pub reputation_score: i32,
}

impl Peer {
    pub fn new(id: PeerID) -> Self {
        Self {
            id,
            state: PeerState::Connecting,
            last_seen: Instant::now(),
            uptime_seconds: 0,
            reputation_score: 0,
        }
    }

    pub fn set_state(&mut self, state: PeerState) {
        self.state = state;
        self.last_seen = Instant::now();
    }

    pub fn update_uptime(&mut self) {
        let elapsed = self.last_seen.elapsed().as_secs();
        self.uptime_seconds += elapsed;
        self.last_seen = Instant::now();
    }

    pub fn is_connected(&self) -> bool {
        self.state == PeerState::Connected
    }

    pub fn is_banned(&self) -> bool {
        self.state == PeerState::Banned
    }
}

/// Peer manager trait
pub trait PeerManager {
    fn add_peer(&mut self, peer: Peer) -> Result<()>;
    fn remove_peer(&mut self, peer_id: &PeerID) -> Result<()>;
    fn get_peer(&self, peer_id: &PeerID) -> Option<&Peer>;
    fn get_peers(&self) -> Vec<&Peer>;
    fn get_connected_peers(&self) -> Vec<&Peer>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_id_creation() {
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        assert_eq!(peer_id.address, "XNTTestAddress");
    }

    #[test]
    fn test_peer_creation() {
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        let peer = Peer::new(peer_id);
        assert_eq!(peer.state, PeerState::Connecting);
        assert!(!peer.is_connected());
    }

    #[test]
    fn test_peer_state_transition() {
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        let mut peer = Peer::new(peer_id);
        peer.set_state(PeerState::Connected);
        assert!(peer.is_connected());
    }
}
