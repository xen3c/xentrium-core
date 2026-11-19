use std::net::SocketAddr;
use std::time::Duration;
use xentrium_network::{
    Message, MessageType, PeerID, TcpTransport, TlsCredentials, TlsTransportConfig,
    XentriumCertVerifier,
};

fn peer_id(addr: SocketAddr, id: &str) -> PeerID {
    PeerID::new(id.to_string(), addr, format!("machine-{}", id))
}

fn tls_transport(node_id: &str) -> TcpTransport {
    let creds = TlsCredentials::generate(node_id).expect("generate TLS creds");
    let tls = TlsTransportConfig::new(&creds).expect("TLS config");
    let mut transport = TcpTransport::new();
    transport.enable_tls(tls);
    transport
}

async fn bind_addr(transport: &mut TcpTransport) -> SocketAddr {
    transport
        .start_listener("127.0.0.1:0".parse().unwrap())
        .await
        .expect("bind listener");
    transport.local_address().unwrap()
}

#[tokio::test]
async fn tls_handshake_success() {
    let mut server = tls_transport("node-alpha");
    let addr = bind_addr(&mut server).await;

    let client = tls_transport("node-beta");
    // TLS handshake requires both sides to be polled concurrently
    let (conn, accepted) = tokio::join!(
        client.connect(peer_id(addr, "node-beta")),
        server.accept(),
    );
    assert!(conn.is_ok(), "TLS client connect should succeed: {:?}", conn.err());
    assert!(accepted.is_ok(), "TLS server accept should succeed: {:?}", accepted.err());
}

#[tokio::test]
async fn tls_handshake_failure_plaintext_client() {
    let mut server = tls_transport("node-alpha");
    let addr = bind_addr(&mut server).await;

    // Plaintext client sends a framed message; TLS acceptor must reject it
    let plain = TcpTransport::new();
    let mut conn = plain.connect(peer_id(addr, "plain")).await.expect("tcp connect");
    conn.send(Message::new(MessageType::Ping, b"x".to_vec(), vec![]))
        .await
        .expect("plaintext send");

    let accepted = tokio::time::timeout(Duration::from_secs(5), server.accept()).await;
    match accepted {
        Ok(Err(_)) => {} // expected: TLS accept failed
        Ok(Ok(_)) => panic!("plaintext client should not complete TLS accept"),
        Err(_) => panic!("accept timed out"),
    }
}

#[tokio::test]
async fn tls_handshake_failure_tls_client_plaintext_server() {
    // TLS client connecting to a plaintext TCP server must fail
    let mut plain_server = TcpTransport::new();
    let addr = bind_addr(&mut plain_server).await;

    let client = tls_transport("node-beta");
    // Plaintext server never completes a TLS handshake — must time out / error
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        client.connect(peer_id(addr, "node-beta")),
    )
    .await;
    match result {
        Ok(Err(_)) => {}             // handshake error — expected
        Err(_) => {}                 // timed out waiting for handshake — acceptable rejection
        Ok(Ok(_)) => panic!("TLS client to plaintext server should not succeed"),
    }
}

#[tokio::test]
async fn certificate_validation() {
    // Valid self-signed cert yields its CN as node identity
    let creds = TlsCredentials::generate("node-cert-test").unwrap();
    let cn = XentriumCertVerifier::cert_node_id(creds.cert_der());
    assert_eq!(cn, Some("node-cert-test".to_string()));

    // Malformed DER is rejected
    assert_eq!(XentriumCertVerifier::cert_node_id(&[0u8; 64]), None);
    assert_eq!(XentriumCertVerifier::cert_node_id(b"not a cert"), None);

    // Expired certificate is rejected (validity window entirely in the past)
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    params.not_before = rcgen::date_time_ymd(2020, 1, 1);
    params.not_after = rcgen::date_time_ymd(2020, 6, 1);
    let expired = params.self_signed(&key_pair).unwrap();
    assert_eq!(
        XentriumCertVerifier::cert_node_id(expired.der()),
        None,
        "expired certificate must be rejected"
    );
}

#[tokio::test]
async fn invalid_certificate_rejection() {
    // A TLS client must reject a peer presenting a malformed/non-TLS endpoint.
    // Server side: raw TCP listener that accepts then immediately sends garbage.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut s, _)) = listener.accept().await {
            use tokio::io::AsyncWriteExt;
            let _ = s.write_all(b"GARBAGE NOT TLS").await;
        }
    });

    let client = tls_transport("node-beta");
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        client.connect(peer_id(addr, "evil")),
    )
    .await;
    match result {
        Ok(Err(_)) => {} // handshake error — expected
        Err(_) => {}     // timeout also constitutes rejection
        Ok(Ok(_)) => panic!("TLS handshake against garbage endpoint must fail"),
    }
}

#[tokio::test]
async fn encrypted_ping_pong() {
    let mut server = tls_transport("node-alpha");
    let addr = bind_addr(&mut server).await;

    let client = tls_transport("node-beta");
    let (client_res, server_res) = tokio::join!(
        client.connect(peer_id(addr, "node-beta")),
        server.accept(),
    );
    let mut client_conn = client_res.expect("connect");
    let mut server_conn = server_res.expect("accept");

    // Verify certificate-bound identities on both ends
    assert_eq!(client_conn.tls_peer_node_id.as_deref(), Some("node-alpha"));
    assert_eq!(server_conn.tls_peer_node_id.as_deref(), Some("node-beta"));

    // Ping over encrypted channel
    let ping = Message::new(MessageType::Ping, b"ping-id".to_vec(), vec![9, 9, 9]);
    client_conn.send(ping).await.expect("send ping");

    let received = tokio::time::timeout(Duration::from_secs(5), server_conn.receive())
        .await
        .expect("receive timeout")
        .expect("receive result")
        .expect("message present");
    assert_eq!(received.message_type, MessageType::Ping);
    assert_eq!(received.payload, vec![9, 9, 9]);

    // Pong back over encrypted channel
    let pong = Message::new(MessageType::Pong, b"ping-id".to_vec(), vec![8, 8, 8]);
    server_conn.send(pong).await.expect("send pong");

    let received = tokio::time::timeout(Duration::from_secs(5), client_conn.receive())
        .await
        .expect("receive timeout")
        .expect("receive result")
        .expect("message present");
    assert_eq!(received.message_type, MessageType::Pong);
    assert_eq!(received.payload, vec![8, 8, 8]);
}
