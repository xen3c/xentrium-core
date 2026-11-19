use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use crate::{PeerID, Peer, PeerState, Result, NetworkError};

/// Discovery trait
pub trait Discovery {
    fn add_bootstrap_peer(&mut self, endpoint: SocketAddr) -> Result<()>;
    fn discover_peers(&mut self) -> Result<Vec<PeerID>>;
    fn announce_peer(&mut self, peer_id: PeerID) -> Result<()>;
    fn get_known_peers(&self) -> Vec<PeerID>;
    fn get_peers(&self) -> Vec<Peer>;
    fn add_peer(&mut self, peer: Peer) -> Result<()>;
    fn remove_peer(&mut self, peer_id: &PeerID) -> Result<()>;
    fn update_peer_state(&mut self, peer_id: &PeerID, state: PeerState) -> Result<()>;
    fn get_peer(&self, peer_id: &PeerID) -> Option<&Peer>;
    fn get_unhealthy_peers(&self) -> Vec<PeerID>;
    fn get_peer_list_for_exchange(&self) -> Vec<PeerID>;
    fn receive_peer_list(&mut self, peer_list: Vec<PeerID>) -> Result<usize>;
}

/// Peer discovery implementation
pub struct BasicDiscovery {
    bootstrap_peers: Vec<SocketAddr>,
    peers: HashMap<PeerID, Peer>,
    last_health_check: Instant,
    health_check_interval: Duration,
}

impl BasicDiscovery {
    pub fn new() -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            peers: HashMap::new(),
            last_health_check: Instant::now(),
            health_check_interval: Duration::from_secs(30),
        }
    }

    pub fn with_health_check_interval(interval: Duration) -> Self {
        Self {
            bootstrap_peers: Vec::new(),
            peers: HashMap::new(),
            last_health_check: Instant::now(),
            health_check_interval: interval,
        }
    }

    fn check_health(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_health_check) >= self.health_check_interval {
            self.last_health_check = now;
            
            // Mark peers as unhealthy if they haven't been seen in 2 minutes
            let unhealthy_threshold = Duration::from_secs(120);
            
            for peer in self.peers.values_mut() {
                if now.duration_since(peer.last_seen) > unhealthy_threshold {
                    peer.set_state(PeerState::Disconnected);
                }
            }
        }
    }
}

impl Discovery for BasicDiscovery {
    fn add_bootstrap_peer(&mut self, endpoint: SocketAddr) -> Result<()> {
        if !self.bootstrap_peers.contains(&endpoint) {
            self.bootstrap_peers.push(endpoint);
        }
        Ok(())
    }

    fn discover_peers(&mut self) -> Result<Vec<PeerID>> {
        self.check_health();
        
        // Return all connected peers
        let connected_peers: Vec<PeerID> = self.peers
            .values()
            .filter(|p| p.is_connected())
            .map(|p| p.id.clone())
            .collect();
        
        Ok(connected_peers)
    }

    fn announce_peer(&mut self, peer_id: PeerID) -> Result<()> {
        let peer = Peer::new(peer_id.clone());
        self.peers.insert(peer_id, peer);
        Ok(())
    }

    fn get_known_peers(&self) -> Vec<PeerID> {
        self.peers.keys().cloned().collect()
    }

    fn get_peers(&self) -> Vec<Peer> {
        self.peers.values().cloned().collect()
    }

    fn add_peer(&mut self, peer: Peer) -> Result<()> {
        self.peers.insert(peer.id.clone(), peer);
        Ok(())
    }

    fn remove_peer(&mut self, peer_id: &PeerID) -> Result<()> {
        self.peers.remove(peer_id)
            .ok_or_else(|| NetworkError::PeerError("Peer not found".to_string()))?;
        Ok(())
    }

    fn update_peer_state(&mut self, peer_id: &PeerID, state: PeerState) -> Result<()> {
        if let Some(peer) = self.peers.get_mut(peer_id) {
            peer.set_state(state);
            Ok(())
        } else {
            Err(NetworkError::PeerError("Peer not found".to_string()))
        }
    }

    fn get_peer(&self, peer_id: &PeerID) -> Option<&Peer> {
        self.peers.get(peer_id)
    }

    fn get_unhealthy_peers(&self) -> Vec<PeerID> {
        self.peers
            .values()
            .filter(|p| !p.is_connected() && !p.is_banned())
            .map(|p| p.id.clone())
            .collect()
    }

    fn get_peer_list_for_exchange(&self) -> Vec<PeerID> {
        // Return up to 10 random connected peers for exchange
        let connected: Vec<PeerID> = self.peers
            .values()
            .filter(|p| p.is_connected())
            .map(|p| p.id.clone())
            .collect();
        
        let limit = std::cmp::min(10, connected.len());
        connected.into_iter().take(limit).collect()
    }

    fn receive_peer_list(&mut self, peer_list: Vec<PeerID>) -> Result<usize> {
        let mut added = 0;
        
        for peer_id in peer_list {
            if !self.peers.contains_key(&peer_id) {
                let peer = Peer::new(peer_id.clone());
                self.peers.insert(peer_id, peer);
                added += 1;
            }
        }
        
        Ok(added)
    }
}

impl Default for BasicDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_discovery_creation() {
        let discovery = BasicDiscovery::new();
        assert_eq!(discovery.get_known_peers().len(), 0);
    }

    #[test]
    fn test_add_bootstrap_peer() {
        let mut discovery = BasicDiscovery::new();
        let addr: SocketAddr = "127.0.0.1:30303".parse().unwrap();
        let result = discovery.add_bootstrap_peer(addr);
        assert!(result.is_ok());
        assert_eq!(discovery.bootstrap_peers.len(), 1);
    }

    #[test]
    fn test_announce_peer() {
        let mut discovery = BasicDiscovery::new();
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        let result = discovery.announce_peer(peer_id.clone());
        assert!(result.is_ok());
        assert_eq!(discovery.get_known_peers().len(), 1);
        assert!(discovery.get_peer(&peer_id).is_some());
    }

    #[test]
    fn test_add_peer() {
        let mut discovery = BasicDiscovery::new();
        let peer_id = PeerID::new(
            "XNTTestAddress2".to_string(),
            "127.0.0.1:30304".parse().unwrap(),
            "machine2".to_string(),
        );
        let peer = Peer::new(peer_id.clone());
        let result = discovery.add_peer(peer);
        assert!(result.is_ok());
        assert_eq!(discovery.get_known_peers().len(), 1);
    }

    #[test]
    fn test_remove_peer() {
        let mut discovery = BasicDiscovery::new();
        let peer_id = PeerID::new(
            "XNTTestAddress3".to_string(),
            "127.0.0.1:30305".parse().unwrap(),
            "machine3".to_string(),
        );
        discovery.announce_peer(peer_id.clone()).unwrap();
        let result = discovery.remove_peer(&peer_id);
        assert!(result.is_ok());
        assert_eq!(discovery.get_known_peers().len(), 0);
    }

    #[test]
    fn test_update_peer_state() {
        let mut discovery = BasicDiscovery::new();
        let peer_id = PeerID::new(
            "XNTTestAddress4".to_string(),
            "127.0.0.1:30306".parse().unwrap(),
            "machine4".to_string(),
        );
        discovery.announce_peer(peer_id.clone()).unwrap();
        let result = discovery.update_peer_state(&peer_id, PeerState::Connected);
        assert!(result.is_ok());
        assert!(discovery.get_peer(&peer_id).unwrap().is_connected());
    }

    #[test]
    fn test_get_unhealthy_peers() {
        let mut discovery = BasicDiscovery::new();
        let peer_id = PeerID::new(
            "XNTTestAddress5".to_string(),
            "127.0.0.1:30307".parse().unwrap(),
            "machine5".to_string(),
        );
        discovery.announce_peer(peer_id.clone()).unwrap();
        discovery.update_peer_state(&peer_id, PeerState::Disconnected).unwrap();
        
        let unhealthy = discovery.get_unhealthy_peers();
        assert_eq!(unhealthy.len(), 1);
        assert!(unhealthy.contains(&peer_id));
    }

    #[test]
    fn test_peer_list_for_exchange() {
        let mut discovery = BasicDiscovery::new();
        
        for i in 0..15 {
            let peer_id = PeerID::new(
                format!("XNTTestAddress{}", i),
                format!("127.0.0.1:{}", 30308 + i).parse().unwrap(),
                format!("machine{}", i),
            );
            discovery.announce_peer(peer_id.clone()).unwrap();
            discovery.update_peer_state(&peer_id, PeerState::Connected).unwrap();
        }
        
        let peer_list = discovery.get_peer_list_for_exchange();
        assert_eq!(peer_list.len(), 10); // Limited to 10
    }

    #[test]
    fn test_receive_peer_list() {
        let mut discovery = BasicDiscovery::new();
        
        let peer_id1 = PeerID::new(
            "XNTTestAddress6".to_string(),
            "127.0.0.1:30308".parse().unwrap(),
            "machine6".to_string(),
        );
        discovery.announce_peer(peer_id1.clone()).unwrap();
        
        let peer_id2 = PeerID::new(
            "XNTTestAddress7".to_string(),
            "127.0.0.1:30309".parse().unwrap(),
            "machine7".to_string(),
        );
        
        let result = discovery.receive_peer_list(vec![peer_id2]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);
        assert_eq!(discovery.get_known_peers().len(), 2);
    }

    #[test]
    fn test_receive_peer_list_duplicates() {
        let mut discovery = BasicDiscovery::new();
        
        let peer_id = PeerID::new(
            "XNTTestAddress8".to_string(),
            "127.0.0.1:30310".parse().unwrap(),
            "machine8".to_string(),
        );
        discovery.announce_peer(peer_id.clone()).unwrap();
        
        let result = discovery.receive_peer_list(vec![peer_id.clone()]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 0); // Duplicate not added
        assert_eq!(discovery.get_known_peers().len(), 1);
    }
}
