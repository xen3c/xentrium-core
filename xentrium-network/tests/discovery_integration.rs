use std::net::SocketAddr;
use xentrium_network::{BasicDiscovery, PeerID, PeerState, Discovery};

#[tokio::test]
async fn test_single_peer_discovery() {
    let mut discovery = BasicDiscovery::new();
    
    let peer_id = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30303".parse().unwrap(),
        "machineA".to_string(),
    );
    
    Discovery::announce_peer(&mut discovery, peer_id.clone()).unwrap();
    Discovery::update_peer_state(&mut discovery, &peer_id, PeerState::Connected).unwrap();
    
    let discovered = Discovery::discover_peers(&mut discovery).unwrap();
    assert_eq!(discovered.len(), 1);
    assert!(discovered.contains(&peer_id));
}

#[tokio::test]
async fn test_bootstrap_discovery() {
    let mut discovery = BasicDiscovery::new();
    
    let addr1: SocketAddr = "127.0.0.1:30303".parse().unwrap();
    let addr2: SocketAddr = "127.0.0.1:30304".parse().unwrap();
    
    Discovery::add_bootstrap_peer(&mut discovery, addr1).unwrap();
    Discovery::add_bootstrap_peer(&mut discovery, addr2).unwrap();
    
    // Bootstrap peers are added (we can't directly access the field, but we can verify no errors)
    // This test just verifies the API works
}

#[tokio::test]
async fn test_peer_exchange() {
    let mut discovery_a = BasicDiscovery::new();
    let mut discovery_b = BasicDiscovery::new();
    
    // Discovery A knows Node B
    let peer_id_b = PeerID::new(
        "NodeB".to_string(),
        "127.0.0.1:30303".parse().unwrap(),
        "machineB".to_string(),
    );
    Discovery::announce_peer(&mut discovery_a, peer_id_b.clone()).unwrap();
    Discovery::update_peer_state(&mut discovery_a, &peer_id_b, PeerState::Connected).unwrap();
    
    // Discovery B knows Node C
    let peer_id_c = PeerID::new(
        "NodeC".to_string(),
        "127.0.0.1:30304".parse().unwrap(),
        "machineC".to_string(),
    );
    Discovery::announce_peer(&mut discovery_b, peer_id_c.clone()).unwrap();
    Discovery::update_peer_state(&mut discovery_b, &peer_id_c, PeerState::Connected).unwrap();
    
    // Exchange peer lists
    let peer_list_a = Discovery::get_peer_list_for_exchange(&discovery_a);
    let peer_list_b = Discovery::get_peer_list_for_exchange(&discovery_b);
    
    let added_to_a = Discovery::receive_peer_list(&mut discovery_a, peer_list_b).unwrap();
    let added_to_b = Discovery::receive_peer_list(&mut discovery_b, peer_list_a).unwrap();
    
    // After exchange, A should know C
    assert!(Discovery::get_peer(&discovery_a, &peer_id_c).is_some());
    assert_eq!(added_to_a, 1);
    
    // After exchange, B should know B (no change) + A
    assert!(Discovery::get_peer(&discovery_b, &peer_id_b).is_some());
    assert_eq!(added_to_b, 1);
}

#[tokio::test]
async fn test_unreachable_peer_removal() {
    let mut discovery = BasicDiscovery::new();
    
    let peer_id = PeerID::new(
        "NodeX".to_string(),
        "127.0.0.1:30305".parse().unwrap(),
        "machineX".to_string(),
    );
    Discovery::announce_peer(&mut discovery, peer_id.clone()).unwrap();
    Discovery::update_peer_state(&mut discovery, &peer_id, PeerState::Connected).unwrap();
    
    // Initially peer is connected
    let discovered = Discovery::discover_peers(&mut discovery).unwrap();
    assert_eq!(discovered.len(), 1);
    
    // Peer becomes unreachable
    Discovery::update_peer_state(&mut discovery, &peer_id, PeerState::Disconnected).unwrap();
    
    // After disconnect, peer should not be in discovered list
    let discovered = Discovery::discover_peers(&mut discovery).unwrap();
    assert_eq!(discovered.len(), 0);
    
    // But peer should still be in unhealthy list
    let unhealthy = Discovery::get_unhealthy_peers(&discovery);
    assert_eq!(unhealthy.len(), 1);
    assert!(unhealthy.contains(&peer_id));
}

#[tokio::test]
async fn test_peer_reconnect() {
    let mut discovery = BasicDiscovery::new();
    
    let peer_id = PeerID::new(
        "NodeY".to_string(),
        "127.0.0.1:30306".parse().unwrap(),
        "machineY".to_string(),
    );
    Discovery::announce_peer(&mut discovery, peer_id.clone()).unwrap();
    
    // Peer connects
    Discovery::update_peer_state(&mut discovery, &peer_id, PeerState::Connected).unwrap();
    assert!(Discovery::discover_peers(&mut discovery).unwrap().contains(&peer_id));
    
    // Peer disconnects
    Discovery::update_peer_state(&mut discovery, &peer_id, PeerState::Disconnected).unwrap();
    assert!(!Discovery::discover_peers(&mut discovery).unwrap().contains(&peer_id));
    
    // Peer reconnects
    Discovery::update_peer_state(&mut discovery, &peer_id, PeerState::Connected).unwrap();
    assert!(Discovery::discover_peers(&mut discovery).unwrap().contains(&peer_id));
}
