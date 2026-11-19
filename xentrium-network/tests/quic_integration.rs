use std::net::SocketAddr;
use xentrium_network::{TcpTransport, PeerID, Message, MessageType};

#[tokio::test]
async fn test_tcp_ping_pong() {
    // Node A: Start listener
    let mut transport_a = TcpTransport::new();
    let addr_a: SocketAddr = "127.0.0.1:30303".parse().unwrap();
    transport_a.start_listener(addr_a).await.unwrap();

    // Node B: Connect to A
    let transport_b = TcpTransport::new();
    let peer_id_a = PeerID::new(
        "NodeA".to_string(),
        addr_a,
        "machineA".to_string(),
    );
    let mut connection_b = transport_b.connect(peer_id_a).await.unwrap();

    // Node A: Accept connection
    let mut connection_a = transport_a.accept().await.unwrap();

    // Node B: Send Ping
    let ping = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"PING".to_vec(),
    );
    connection_b.send(ping).await.unwrap();

    // Node A: Receive Ping
    let received = connection_a.receive().await.unwrap();
    assert!(received.is_some());
    let msg = received.unwrap();
    assert_eq!(msg.message_type, MessageType::Ping);
    assert_eq!(msg.payload, b"PING".to_vec());

    // Node A: Send Pong
    let pong = Message::new(
        MessageType::Pong,
        vec![4, 5, 6],
        b"PONG".to_vec(),
    );
    connection_a.send(pong).await.unwrap();

    // Node B: Receive Pong
    let received = connection_b.receive().await.unwrap();
    assert!(received.is_some());
    let msg = received.unwrap();
    assert_eq!(msg.message_type, MessageType::Pong);
    assert_eq!(msg.payload, b"PONG".to_vec());

    // Graceful disconnect
    connection_a.close().await.unwrap();
    connection_b.close().await.unwrap();
}

#[tokio::test]
async fn test_tcp_reconnect() {
    // Node A: Start listener
    let mut transport_a = TcpTransport::new();
    let addr_a: SocketAddr = "127.0.0.1:30304".parse().unwrap();
    transport_a.start_listener(addr_a).await.unwrap();

    // First connection
    let transport_b = TcpTransport::new();
    let peer_id_a = PeerID::new(
        "NodeA".to_string(),
        addr_a,
        "machineA".to_string(),
    );
    let mut connection_b = transport_b.connect(peer_id_a.clone()).await.unwrap();
    let mut connection_a = transport_a.accept().await.unwrap();

    // Send Ping
    let ping = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"PING".to_vec(),
    );
    connection_b.send(ping).await.unwrap();
    let received = connection_a.receive().await.unwrap();
    assert!(received.is_some());

    // Close connection
    connection_a.close().await.unwrap();
    connection_b.close().await.unwrap();

    // Reconnect
    let mut connection_b2 = transport_b.connect(peer_id_a).await.unwrap();
    let mut connection_a2 = transport_a.accept().await.unwrap();

    // Send Ping again
    let ping2 = Message::new(
        MessageType::Ping,
        vec![1, 2, 3],
        b"PING2".to_vec(),
    );
    connection_b2.send(ping2).await.unwrap();
    let received = connection_a2.receive().await.unwrap();
    assert!(received.is_some());
    let msg = received.unwrap();
    assert_eq!(msg.payload, b"PING2".to_vec());

    connection_a2.close().await.unwrap();
    connection_b2.close().await.unwrap();
}

#[tokio::test]
async fn test_tcp_multiple_messages() {
    // Node A: Start listener
    let mut transport_a = TcpTransport::new();
    let addr_a: SocketAddr = "127.0.0.1:30305".parse().unwrap();
    transport_a.start_listener(addr_a).await.unwrap();

    // Node B: Connect
    let transport_b = TcpTransport::new();
    let peer_id_a = PeerID::new(
        "NodeA".to_string(),
        addr_a,
        "machineA".to_string(),
    );
    let mut connection_b = transport_b.connect(peer_id_a).await.unwrap();
    let mut connection_a = transport_a.accept().await.unwrap();

    // Send multiple messages
    for i in 0..5 {
        let msg = Message::new(
            MessageType::Ping,
            vec![i],
            format!("MSG{}", i).into_bytes(),
        );
        connection_b.send(msg).await.unwrap();
    }

    // Receive all messages
    for i in 0..5 {
        let received = connection_a.receive().await.unwrap();
        assert!(received.is_some());
        let msg = received.unwrap();
        assert_eq!(msg.payload, format!("MSG{}", i).into_bytes());
    }

    connection_a.close().await.unwrap();
    connection_b.close().await.unwrap();
}
