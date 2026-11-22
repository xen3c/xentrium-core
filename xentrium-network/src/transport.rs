use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use crate::{PeerID, Message, MessageType, Result, NetworkError};
use crate::tls::{TlsTransportConfig, XentriumCertVerifier};

/// Unified async stream trait for plain TCP and TLS connections
pub trait IoStream: AsyncRead + AsyncWrite + Unpin + Send + Sync {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + Sync> IoStream for T {}

/// Connection trait
pub trait NetworkConnection {
    fn peer_id(&self) -> &PeerID;
    fn is_connected(&self) -> bool;
}

/// Transport trait
pub trait Transport {
    fn local_endpoint(&self) -> Option<SocketAddr>;
}

/// Maximum allowed message payload size (16 MiB) — rejects oversized frames
/// before allocation to prevent memory-exhaustion DoS.
pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;

/// TCP/TLS connection wrapper
pub struct TcpConnection {
    peer_id: PeerID,
    stream: Box<dyn IoStream>,
    /// Buffer of received-but-unparsed bytes; keeps receive() cancel-safe
    recv_buf: Vec<u8>,
    /// Node identity extracted from peer's TLS certificate (if TLS active)
    pub tls_peer_node_id: Option<String>,
    /// Authenticated node identity from verified handshake (set by node runtime)
    pub auth_node_id: Option<String>,
    /// Authenticated public key from verified handshake
    pub auth_pubkey: Option<Vec<u8>>,
}

impl TcpConnection {
    pub fn new<S: IoStream + 'static>(peer_id: PeerID, stream: S) -> Self {
        Self {
            peer_id,
            stream: Box::new(stream),
            recv_buf: Vec::new(),
            tls_peer_node_id: None,
            auth_node_id: None,
            auth_pubkey: None,
        }
    }

    fn with_tls_peer(mut self, node_id: Option<String>) -> Self {
        self.tls_peer_node_id = node_id;
        self
    }
}

impl NetworkConnection for TcpConnection {
    fn peer_id(&self) -> &PeerID {
        &self.peer_id
    }

    fn is_connected(&self) -> bool {
        true // TCP streams are persistent
    }
}

/// TCP/TLS transport implementation
pub struct TcpTransport {
    listener: Option<TcpListener>,
    local_address: Option<SocketAddr>,
    tls: Option<TlsTransportConfig>,
}

impl TcpTransport {
    pub fn new() -> Self {
        Self {
            listener: None,
            local_address: None,
            tls: None,
        }
    }

    /// Enable TLS on this transport (both accept and connect paths)
    pub fn enable_tls(&mut self, tls: TlsTransportConfig) {
        self.tls = Some(tls);
    }

    /// Start listener on the given address
    pub async fn start_listener(&mut self, address: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(address)
            .await
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to bind: {}", e)))?;

        self.local_address = Some(listener.local_addr()
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to get local address: {}", e)))?);
        self.listener = Some(listener);

        Ok(())
    }

    /// Connect to a remote peer (TLS if enabled)
    pub async fn connect(&self, peer_id: PeerID) -> Result<TcpConnection> {
        let stream = TcpStream::connect(peer_id.endpoint)
            .await
            .map_err(|e| NetworkError::ConnectionError(format!("Failed to connect: {}", e)))?;

        if let Some(tls) = &self.tls {
            let server_name = TlsTransportConfig::server_name();
            let tls_stream = tls.connector.connect(server_name, stream).await
                .map_err(|e| NetworkError::ConnectionError(format!("TLS handshake failed: {}", e)))?;
            // Extract peer node identity from certificate
            let peer_node_id = tls_stream.get_ref().1.peer_certificates()
                .and_then(|certs| certs.first())
                .and_then(|c| XentriumCertVerifier::cert_node_id(c.as_ref()));
            Ok(TcpConnection::new(peer_id, tls_stream).with_tls_peer(peer_node_id))
        } else {
            Ok(TcpConnection::new(peer_id, stream))
        }
    }

    /// Accept a raw TCP connection without performing the TLS handshake
    pub async fn accept_tcp(&self) -> Result<(TcpStream, SocketAddr)> {
        let listener = self.listener.as_ref()
            .ok_or_else(|| NetworkError::ConnectionError("Not listening".to_string()))?;

        listener.accept()
            .await
            .map_err(|e| NetworkError::ConnectionError(format!("Accept failed: {}", e)))
    }

    /// Upgrade an accepted TCP stream into a connection (performs TLS handshake if enabled)
    pub async fn upgrade_connection(&self, stream: TcpStream, addr: SocketAddr) -> Result<TcpConnection> {
        let peer_id = PeerID::new(
            "unknown".to_string(),
            addr,
            "unknown".to_string(),
        );

        if let Some(tls) = &self.tls {
            let tls_stream = tls.acceptor.accept(stream).await
                .map_err(|e| NetworkError::ConnectionError(format!("TLS accept failed: {}", e)))?;
            let peer_node_id = tls_stream.get_ref().1.peer_certificates()
                .and_then(|certs| certs.first())
                .and_then(|c| XentriumCertVerifier::cert_node_id(c.as_ref()));
            Ok(TcpConnection::new(peer_id, tls_stream).with_tls_peer(peer_node_id))
        } else {
            Ok(TcpConnection::new(peer_id, stream))
        }
    }

    /// Accept incoming connection (TLS if enabled)
    pub async fn accept(&self) -> Result<TcpConnection> {
        let (stream, addr) = self.accept_tcp().await?;
        self.upgrade_connection(stream, addr).await
    }

    /// Get local address
    pub fn local_address(&self) -> Option<SocketAddr> {
        self.local_address
    }
}

impl Transport for TcpTransport {
    fn local_endpoint(&self) -> Option<SocketAddr> {
        self.local_address
    }
}

/// Connection implementation for TCP
impl TcpConnection {
    /// Send a message
    pub async fn send(&mut self, message: Message) -> Result<()> {
        let serialized = serialize_message(&message)?;
        let len = serialized.len() as u32;

        // Send length prefix
        self.stream.write_all(&len.to_be_bytes()).await
            .map_err(|e| NetworkError::IoError(e))?;

        // Send message
        self.stream.write_all(&serialized).await
            .map_err(|e| NetworkError::IoError(e))?;

        Ok(())
    }

    /// Receive a message
    ///
    /// Cancel-safe: incoming bytes are appended to `recv_buf` before parsing,
    /// and each poll performs at most one `read()` on the stream. Dropping this
    /// future mid-frame (e.g. under `tokio::time::timeout`) cannot desynchronize
    /// the stream — partially received frames simply wait in the buffer.
    pub async fn receive(&mut self) -> Result<Option<Message>> {
        loop {
            if self.recv_buf.len() >= 4 {
                let len = u32::from_be_bytes([
                    self.recv_buf[0], self.recv_buf[1], self.recv_buf[2], self.recv_buf[3],
                ]) as usize;

                // C2: reject oversized frames before allocating
                if len == 0 || len > MAX_MESSAGE_SIZE {
                    return Err(NetworkError::ProtocolError(format!(
                        "Message size {} exceeds limit {}", len, MAX_MESSAGE_SIZE)));
                }

                if self.recv_buf.len() >= 4 + len {
                    let payload: Vec<u8> = self.recv_buf.drain(..4 + len).skip(4).collect();
                    return Ok(Some(deserialize_message(&payload)?));
                }
            }

            let mut chunk = [0u8; 8192];
            let n = self.stream.read(&mut chunk).await
                .map_err(NetworkError::IoError)?;
            if n == 0 {
                return Err(NetworkError::ConnectionError("Connection closed".to_string()));
            }
            self.recv_buf.extend_from_slice(&chunk[..n]);
        }
    }

    /// Close connection
    pub async fn close(&mut self) -> Result<()> {
        self.stream.shutdown().await
            .map_err(|e| NetworkError::IoError(e))?;
        Ok(())
    }
}

/// Serialize message to bytes
fn serialize_message(message: &Message) -> Result<Vec<u8>> {
    let message_type_byte = match message.message_type {
        MessageType::Handshake => 0,
        MessageType::Transaction => 1,
        MessageType::StateUpdate => 2,
        MessageType::SnapshotOffer => 3,
        MessageType::SnapshotRequest => 4,
        MessageType::SnapshotData => 5,
        MessageType::CommitteeVote => 6,
        MessageType::Heartbeat => 7,
        MessageType::PeerList => 8,
        MessageType::Ping => 9,
        MessageType::Pong => 10,
        MessageType::ValidatorRegistration => 11,
        MessageType::ValidatorStatusUpdate => 12,
        MessageType::CommitteeSelection => 13,
    };

    let mut buffer = Vec::new();
    buffer.push(message_type_byte);
    buffer.extend_from_slice(&message.timestamp.to_be_bytes());
    buffer.extend_from_slice(&(message.sender.len() as u32).to_be_bytes());
    buffer.extend_from_slice(&message.sender);
    buffer.extend_from_slice(&(message.payload.len() as u32).to_be_bytes());
    buffer.extend_from_slice(&message.payload);

    Ok(buffer)
}

/// Deserialize message from bytes
fn deserialize_message(data: &[u8]) -> Result<Message> {
    if data.len() < 9 {
        return Err(NetworkError::ProtocolError("Invalid message length".to_string()));
    }

    let message_type_byte = data[0];
    let message_type = match message_type_byte {
        0 => MessageType::Handshake,
        1 => MessageType::Transaction,
        2 => MessageType::StateUpdate,
        3 => MessageType::SnapshotOffer,
        4 => MessageType::SnapshotRequest,
        5 => MessageType::SnapshotData,
        6 => MessageType::CommitteeVote,
        7 => MessageType::Heartbeat,
        8 => MessageType::PeerList,
        9 => MessageType::Ping,
        10 => MessageType::Pong,
        11 => MessageType::ValidatorRegistration,
        12 => MessageType::ValidatorStatusUpdate,
        13 => MessageType::CommitteeSelection,
        _ => return Err(NetworkError::ProtocolError("Unknown message type".to_string())),
    };

    let timestamp = u64::from_be_bytes([data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8]]);

    let mut offset = 9;
    let sender_len = u32::from_be_bytes([
        data[offset], data[offset+1], data[offset+2], data[offset+3]
    ]) as usize;
    offset += 4;

    if data.len() < offset + sender_len {
        return Err(NetworkError::ProtocolError("Invalid sender length".to_string()));
    }

    let sender = data[offset..offset+sender_len].to_vec();
    offset += sender_len;

    let payload_len = u32::from_be_bytes([
        data[offset], data[offset+1], data[offset+2], data[offset+3]
    ]) as usize;
    offset += 4;

    if data.len() < offset + payload_len {
        return Err(NetworkError::ProtocolError("Invalid payload length".to_string()));
    }

    let payload = data[offset..offset+payload_len].to_vec();

    Ok(Message {
        message_type,
        timestamp,
        sender,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_ping_message() {
        let message = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let serialized = serialize_message(&message);
        assert!(serialized.is_ok());
    }

    #[test]
    fn test_deserialize_ping_message() {
        let message = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let serialized = serialize_message(&message).unwrap();
        let deserialized = deserialize_message(&serialized);
        assert!(deserialized.is_ok());
        let msg = deserialized.unwrap();
        assert_eq!(msg.message_type, MessageType::Ping);
    }

    #[test]
    fn test_serialize_pong_message() {
        let message = Message::new(
            MessageType::Pong,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let serialized = serialize_message(&message);
        assert!(serialized.is_ok());
    }

    #[test]
    fn test_deserialize_pong_message() {
        let message = Message::new(
            MessageType::Pong,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let serialized = serialize_message(&message).unwrap();
        let deserialized = deserialize_message(&serialized);
        assert!(deserialized.is_ok());
        let msg = deserialized.unwrap();
        assert_eq!(msg.message_type, MessageType::Pong);
    }
}
