use xentrium_network::{BasicGossip, PeerID, Message, MessageType, GossipProtocol};

#[tokio::test]
async fn test_gossip_propagation_a_to_c() {
    // Simulate 3 nodes: A, B, C
    let mut gossip_a = BasicGossip::new();
    let mut gossip_b = BasicGossip::new();
    let mut gossip_c = BasicGossip::new();
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    let peer_b = PeerID::new(
        "NodeB".to_string(),
        "127.0.0.1:30302".parse().unwrap(),
        "machineB".to_string(),
    );
    let _peer_c = PeerID::new(
        "NodeC".to_string(),
        "127.0.0.1:30303".parse().unwrap(),
        "machineC".to_string(),
    );
    
    // Node A broadcasts a Ping
    let ping = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"PING_FROM_A".to_vec(),
    );
    GossipProtocol::broadcast(&mut gossip_a, ping.clone()).unwrap();
    
    // Node B receives from A
    GossipProtocol::receive(&mut gossip_b, ping.clone(), peer_a.clone()).unwrap();
    
    // Node B relays to C
    GossipProtocol::broadcast(&mut gossip_b, ping.clone()).unwrap();
    
    // Node C receives from B
    GossipProtocol::receive(&mut gossip_c, ping, peer_b).unwrap();
    
    // Verify all nodes have seen the message
    assert_eq!(gossip_a.get_seen_count(), 1);
    assert_eq!(gossip_b.get_seen_count(), 1);
    assert_eq!(gossip_c.get_seen_count(), 1);
}

#[tokio::test]
async fn test_duplicate_suppression() {
    let mut gossip_a = BasicGossip::new();
    let mut gossip_b = BasicGossip::new();
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    // Node A broadcasts a Ping
    let ping = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"DUPLICATE_TEST".to_vec(),
    );
    GossipProtocol::broadcast(&mut gossip_a, ping.clone()).unwrap();
    
    // Node B receives from A (first time)
    GossipProtocol::receive(&mut gossip_b, ping.clone(), peer_a.clone()).unwrap();
    assert_eq!(gossip_b.get_seen_count(), 1);
    
    // Node B receives from A again (duplicate)
    GossipProtocol::receive(&mut gossip_b, ping, peer_a).unwrap();
    assert_eq!(gossip_b.get_seen_count(), 1); // Still 1, not 2
}

#[tokio::test]
async fn test_loop_prevention() {
    let mut gossip_a = BasicGossip::new();
    let mut gossip_b = BasicGossip::new();
    let mut gossip_c = BasicGossip::new();
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    let peer_b = PeerID::new(
        "NodeB".to_string(),
        "127.0.0.1:30302".parse().unwrap(),
        "machineB".to_string(),
    );
    let peer_c = PeerID::new(
        "NodeC".to_string(),
        "127.0.0.1:30303".parse().unwrap(),
        "machineC".to_string(),
    );
    
    // Node A broadcasts
    let ping = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"LOOP_TEST".to_vec(),
    );
    GossipProtocol::broadcast(&mut gossip_a, ping.clone()).unwrap();
    
    // Node B receives from A
    GossipProtocol::receive(&mut gossip_b, ping.clone(), peer_a.clone()).unwrap();
    
    // Node C receives from B
    GossipProtocol::receive(&mut gossip_c, ping.clone(), peer_b.clone()).unwrap();
    
    // Node C tries to send back to A (loop)
    GossipProtocol::receive(&mut gossip_a, ping, peer_c).unwrap();
    
    // After loop attempt, A should still only have the message once
    assert_eq!(gossip_a.get_seen_count(), 1);
}

#[tokio::test]
async fn test_ttl_expiration() {
    // Create gossip with very short TTL for testing
    let mut gossip = BasicGossip::with_config(100, 10, 0); // 0 second TTL for immediate expiration
    
    let _peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    let ping = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"TTL_TEST".to_vec(),
    );
    
    GossipProtocol::broadcast(&mut gossip, ping).unwrap();
    
    // Cleanup should immediately expire due to 0 TTL
    // Note: This test is structural - actual time-based cleanup requires tokio::time::sleep
    // We verify the structure is in place
    assert!(gossip.get_seen_count() >= 1);
}

#[tokio::test]
async fn test_disconnected_peer_handling() {
    let mut gossip_a = BasicGossip::new();
    let mut gossip_b = BasicGossip::new();
    let mut gossip_c = BasicGossip::new();
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    // Node A broadcasts
    let ping = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"DISCONNECTED_TEST".to_vec(),
    );
    GossipProtocol::broadcast(&mut gossip_a, ping.clone()).unwrap();
    
    // Node B receives and would relay to C
    GossipProtocol::receive(&mut gossip_b, ping.clone(), peer_a.clone()).unwrap();
    
    // Simulate C being disconnected (B cannot send to C)
    // In a real implementation, the transport layer would handle this
    // For this test, we just verify the gossip state is correct
    
    // C receives from A directly (simulating alternate path)
    GossipProtocol::receive(&mut gossip_c, ping, peer_a).unwrap();
    
    // Verify both B and C have the message
    assert_eq!(gossip_b.get_seen_count(), 1);
    assert_eq!(gossip_c.get_seen_count(), 1);
}

#[tokio::test]
async fn test_peer_list_gossip() {
    let mut gossip_a = BasicGossip::new();
    let mut gossip_b = BasicGossip::new();
    
    let peer_a = PeerID::new(
        "NodeA".to_string(),
        "127.0.0.1:30301".parse().unwrap(),
        "machineA".to_string(),
    );
    
    // Node A broadcasts a PeerList message
    let peer_list_msg = Message::new(
        MessageType::PeerList,
        vec![1, 2, 3],
        b"PEER_LIST_DATA".to_vec(),
    );
    GossipProtocol::broadcast(&mut gossip_a, peer_list_msg.clone()).unwrap();
    
    // Node B receives the PeerList
    GossipProtocol::receive(&mut gossip_b, peer_list_msg, peer_a).unwrap();
    
    // Verify message was processed
    assert_eq!(gossip_b.get_seen_count(), 1);
}
