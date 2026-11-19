use xentrium_network::{TcpTransport, PeerID, Message, MessageType};

#[tokio::test]
async fn test_direct_tcp_ping_pong() {
    let mut transport_a = TcpTransport::new();
    let mut transport_b = TcpTransport::new();
    
    let addr_a: std::net::SocketAddr = "127.0.0.1:50000".parse().unwrap();
    let addr_b: std::net::SocketAddr = "127.0.0.1:50001".parse().unwrap();
    
    // Start listener on A
    transport_a.start_listener(addr_a).await.unwrap();
    println!("Transport A listening on {}", addr_a);
    
    // Start listener on B
    transport_b.start_listener(addr_b).await.unwrap();
    println!("Transport B listening on {}", addr_b);
    
    // B connects to A
    let peer_id_a = PeerID::new("NodeA".to_string(), addr_a, "machineA".to_string());
    let mut connection_ba = transport_b.connect(peer_id_a).await.unwrap();
    println!("B connected to A");
    
    // Send Ping from B to A
    let ping = Message::new(
        MessageType::Ping,
        b"NodeB".to_vec(),
        b"PING".to_vec(),
    );
    connection_ba.send(ping).await.unwrap();
    println!("B sent Ping to A");
    
    // A accepts connection and receives Ping
    let mut connection_ab = transport_a.accept().await.unwrap();
    println!("A accepted connection from B");
    
    let received = connection_ab.receive().await.unwrap();
    println!("A received: {:?}", received.unwrap().message_type);
    
    // A sends Pong
    let pong = Message::new(
        MessageType::Pong,
        b"NodeA".to_vec(),
        b"PONG".to_vec(),
    );
    connection_ab.send(pong).await.unwrap();
    println!("A sent Pong to B");
    
    // B receives Pong
    let received = connection_ba.receive().await.unwrap();
    println!("B received: {:?}", received.unwrap().message_type);
    
    println!("Test passed!");
}
