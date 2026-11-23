use crate::Result;
use crate::peer::PeerID;
use std::io::{Cursor, Read, Write};
use xentrium_shared::{Address, ValidatorInfo, ValidatorState, Snapshot};

/// Message types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    Handshake,
    Transaction,
    StateUpdate,
    SnapshotOffer,
    SnapshotRequest,
    SnapshotData,
    CommitteeVote,
    Heartbeat,
    PeerList,
    Ping,
    Pong,
    ValidatorRegistration,
    ValidatorStatusUpdate,
    CommitteeSelection,
}

/// Network message
#[derive(Debug, Clone)]
pub struct Message {
    pub message_type: MessageType,
    pub timestamp: u64,
    pub sender: Vec<u8>,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(message_type: MessageType, sender: Vec<u8>, payload: Vec<u8>) -> Self {
        Self {
            message_type,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            sender,
            payload,
        }
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        cursor.write_all(&[self.message_type.clone() as u8])?;
        cursor.write_all(&self.timestamp.to_be_bytes())?;
        cursor.write_all(&(self.sender.len() as u16).to_be_bytes())?;
        cursor.write_all(&self.sender)?;
        cursor.write_all(&(self.payload.len() as u32).to_be_bytes())?;
        cursor.write_all(&self.payload)?;
        
        Ok(buffer)
    }

    pub fn deserialize(data: &[u8]) -> Result<Message> {
        let mut cursor = Cursor::new(data);
        
        let mut type_byte = [0u8; 1];
        cursor.read_exact(&mut type_byte)?;
        let message_type = match type_byte[0] {
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
            _ => return Err(crate::NetworkError::ProtocolError("Invalid message type".to_string()).into()),
        };
        
        let mut timestamp_bytes = [0u8; 8];
        cursor.read_exact(&mut timestamp_bytes)?;
        let timestamp = u64::from_be_bytes(timestamp_bytes);
        
        let mut sender_len_bytes = [0u8; 2];
        cursor.read_exact(&mut sender_len_bytes)?;
        let sender_len = u16::from_be_bytes(sender_len_bytes) as usize;
        
        let mut sender = vec![0u8; sender_len];
        cursor.read_exact(&mut sender)?;
        
        let mut payload_len_bytes = [0u8; 4];
        cursor.read_exact(&mut payload_len_bytes)?;
        let payload_len = u32::from_be_bytes(payload_len_bytes) as usize;

        // Reject oversized payloads before allocation
        if payload_len > crate::transport::MAX_MESSAGE_SIZE {
            return Err(crate::NetworkError::ProtocolError(
                format!("Payload size {} exceeds limit", payload_len)).into());
        }

        let mut payload = vec![0u8; payload_len];
        cursor.read_exact(&mut payload)?;
        
        Ok(Message {
            message_type,
            timestamp,
            sender,
            payload,
        })
    }

    /// Serialize a list of PeerIDs into a payload for PeerList message
    pub fn serialize_peer_list(peer_list: &[PeerID]) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        cursor.write_all(&(peer_list.len() as u16).to_be_bytes())?;
        
        for peer_id in peer_list {
            cursor.write_all(&(peer_id.address.len() as u16).to_be_bytes())?;
            cursor.write_all(peer_id.address.as_bytes())?;
            
            // Address family (1 byte: 4 for IPv4, 6 for IPv6)
            match peer_id.endpoint {
                std::net::SocketAddr::V4(addr) => {
                    cursor.write_all(&[4u8])?;
                    cursor.write_all(&addr.ip().octets())?;
                    cursor.write_all(&addr.port().to_be_bytes())?;
                }
                std::net::SocketAddr::V6(addr) => {
                    cursor.write_all(&[6u8])?;
                    cursor.write_all(&addr.ip().octets())?;
                    cursor.write_all(&addr.port().to_be_bytes())?;
                }
            }
            
            cursor.write_all(&(peer_id.machine_id.len() as u16).to_be_bytes())?;
            cursor.write_all(peer_id.machine_id.as_bytes())?;
        }
        
        Ok(buffer)
    }

    /// Deserialize a PeerList payload into a list of PeerIDs
    pub fn deserialize_peer_list(payload: &[u8]) -> Result<Vec<PeerID>> {
        let mut cursor = Cursor::new(payload);
        
        let mut count_bytes = [0u8; 2];
        cursor.read_exact(&mut count_bytes)?;
        let count = u16::from_be_bytes(count_bytes) as usize;
        
        let mut peer_list = Vec::with_capacity(count);
        
        for _ in 0..count {
            let mut addr_len_bytes = [0u8; 2];
            cursor.read_exact(&mut addr_len_bytes)?;
            let addr_len = u16::from_be_bytes(addr_len_bytes) as usize;
            let mut addr_bytes = vec![0u8; addr_len];
            cursor.read_exact(&mut addr_bytes)?;
            let address = String::from_utf8(addr_bytes)
                .map_err(|e| crate::NetworkError::ProtocolError(format!("Invalid UTF-8: {}", e)))?;
            
            // Address family
            let mut family_byte = [0u8; 1];
            cursor.read_exact(&mut family_byte)?;
            
            let ip = match family_byte[0] {
                4 => {
                    let mut ip_bytes = [0u8; 4];
                    cursor.read_exact(&mut ip_bytes)?;
                    std::net::IpAddr::V4(std::net::Ipv4Addr::from(ip_bytes))
                }
                6 => {
                    let mut ip_bytes = [0u8; 16];
                    cursor.read_exact(&mut ip_bytes)?;
                    std::net::IpAddr::V6(std::net::Ipv6Addr::from(ip_bytes))
                }
                _ => return Err(crate::NetworkError::ProtocolError("Invalid address family".to_string()).into()),
            };
            
            let mut port_bytes = [0u8; 2];
            cursor.read_exact(&mut port_bytes)?;
            let port = u16::from_be_bytes(port_bytes);
            let endpoint = std::net::SocketAddr::new(ip, port);
            
            let mut machine_len_bytes = [0u8; 2];
            cursor.read_exact(&mut machine_len_bytes)?;
            let machine_len = u16::from_be_bytes(machine_len_bytes) as usize;
            let mut machine_bytes = vec![0u8; machine_len];
            cursor.read_exact(&mut machine_bytes)?;
            let machine_id = String::from_utf8(machine_bytes)
                .map_err(|e| crate::NetworkError::ProtocolError(format!("Invalid UTF-8: {}", e)))?;
            
            peer_list.push(PeerID::new(address, endpoint, machine_id));
        }
        
        Ok(peer_list)
    }

    /// Serialize a ValidatorRegistration message payload
    pub fn serialize_validator_registration(validator: &ValidatorInfo) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        cursor.write_all(&(validator.address.len() as u16).to_be_bytes())?;
        cursor.write_all(validator.address.as_bytes())?;
        
        cursor.write_all(&(validator.balance.to_be_bytes()))?;
        
        // Serialize state as u8: 0=Pending, 1=Active, 2=Suspended
        let state_byte = match validator.state {
            ValidatorState::Pending => 0u8,
            ValidatorState::Active => 1u8,
            ValidatorState::Suspended { .. } => 2u8,
        };
        cursor.write_all(&[state_byte])?;
        
        cursor.write_all(&(validator.reputation_score.to_be_bytes()))?;
        
        cursor.write_all(&(validator.uptime_score.to_be_bytes()))?;
        
        Ok(buffer)
    }

    /// Deserialize a ValidatorRegistration message payload
    pub fn deserialize_validator_registration(payload: &[u8]) -> Result<ValidatorInfo> {
        let mut cursor = Cursor::new(payload);
        
        let mut addr_len_bytes = [0u8; 2];
        cursor.read_exact(&mut addr_len_bytes)?;
        let addr_len = u16::from_be_bytes(addr_len_bytes) as usize;
        let mut addr_bytes = vec![0u8; addr_len];
        cursor.read_exact(&mut addr_bytes)?;
        let address = String::from_utf8(addr_bytes)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Invalid UTF-8: {}", e)))?;
        
        let mut balance_bytes = [0u8; 8];
        cursor.read_exact(&mut balance_bytes)?;
        let balance = i64::from_be_bytes(balance_bytes);
        
        let mut state_byte = [0u8; 1];
        cursor.read_exact(&mut state_byte)?;
        let state = match state_byte[0] {
            0 => ValidatorState::Pending,
            1 => ValidatorState::Active,
            2 => ValidatorState::Suspended { until: 0 },
            _ => return Err(crate::NetworkError::ProtocolError("Invalid validator state".to_string()).into()),
        };
        
        let mut reputation_bytes = [0u8; 8];
        cursor.read_exact(&mut reputation_bytes)?;
        let reputation_score = i64::from_be_bytes(reputation_bytes);
        
        let mut uptime_bytes = [0u8; 4];
        cursor.read_exact(&mut uptime_bytes)?;
        let uptime_score = i32::from_be_bytes(uptime_bytes);
        
        let is_active = state == ValidatorState::Active;
        
        Ok(ValidatorInfo {
            address,
            public_key: vec![],
            machine_id: String::new(),
            balance,
            state,
            enabled_at: None,
            uptime_score,
            reputation_score,
            validator_weight: 0.0,
            last_seen: 0,
            consecutive_uptime_weeks: 0,
            committees_participated: 0,
            votes_cast: 0,
            votes_correct: 0,
            endpoint: String::new(),
            is_active,
        })
    }

    /// Serialize a CommitteeSelection message payload
    pub fn serialize_committee_selection(members: &[Address], state_number: i64) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        cursor.write_all(&(members.len() as u16).to_be_bytes())?;
        
        for member in members {
            cursor.write_all(&(member.len() as u16).to_be_bytes())?;
            cursor.write_all(member.as_bytes())?;
        }
        
        cursor.write_all(&state_number.to_be_bytes())?;
        
        Ok(buffer)
    }

    /// Deserialize a CommitteeSelection message payload
    pub fn deserialize_committee_selection(payload: &[u8]) -> Result<(Vec<Address>, i64)> {
        let mut cursor = Cursor::new(payload);
        
        let mut count_bytes = [0u8; 2];
        cursor.read_exact(&mut count_bytes)?;
        let count = u16::from_be_bytes(count_bytes) as usize;
        
        let mut members = Vec::with_capacity(count);
        
        for _ in 0..count {
            let mut addr_len_bytes = [0u8; 2];
            cursor.read_exact(&mut addr_len_bytes)?;
            let addr_len = u16::from_be_bytes(addr_len_bytes) as usize;
            let mut addr_bytes = vec![0u8; addr_len];
            cursor.read_exact(&mut addr_bytes)?;
            let address = String::from_utf8(addr_bytes)
                .map_err(|e| crate::NetworkError::ProtocolError(format!("Invalid UTF-8: {}", e)))?;
            members.push(address);
        }
        
        let mut state_bytes = [0u8; 8];
        cursor.read_exact(&mut state_bytes)?;
        let state_number = i64::from_be_bytes(state_bytes);
        
        Ok((members, state_number))
    }

    /// Serialize snapshot metadata for SnapshotOffer message
    pub fn serialize_snapshot_metadata(snapshot: &Snapshot) -> Result<Vec<u8>> {
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        cursor.write_all(&snapshot.state_number.to_be_bytes())?;
        
        cursor.write_all(&(snapshot.state_hash.len() as u16).to_be_bytes())?;
        cursor.write_all(snapshot.state_hash.as_bytes())?;
        
        cursor.write_all(&(snapshot.snapshot_hash.len() as u16).to_be_bytes())?;
        cursor.write_all(snapshot.snapshot_hash.as_bytes())?;
        
        cursor.write_all(&snapshot.timestamp.to_be_bytes())?;
        
        cursor.write_all(&(snapshot.accounts.len() as u32).to_be_bytes())?;
        
        cursor.write_all(&(snapshot.active_validators.len() as u16).to_be_bytes())?;
        
        cursor.write_all(&(snapshot.pending_validators.len() as u16).to_be_bytes())?;
        
        cursor.write_all(&snapshot.reward_pool_balance.to_be_bytes())?;
        
        cursor.write_all(&snapshot.last_reward_distribution.to_be_bytes())?;
        
        cursor.write_all(&(snapshot.active_proposals.len() as u16).to_be_bytes())?;
        
        cursor.write_all(&(snapshot.proposal_votes.len() as u32).to_be_bytes())?;
        
        Ok(buffer)
    }

    /// Deserialize snapshot metadata from SnapshotOffer payload
    pub fn deserialize_snapshot_metadata(payload: &[u8]) -> Result<(i64, String, String, i64, usize, usize, usize, i64, i64, usize, usize)> {
        let mut cursor = Cursor::new(payload);
        
        let mut state_bytes = [0u8; 8];
        cursor.read_exact(&mut state_bytes)?;
        let state_number = i64::from_be_bytes(state_bytes);
        
        let mut hash_len_bytes = [0u8; 2];
        cursor.read_exact(&mut hash_len_bytes)?;
        let hash_len = u16::from_be_bytes(hash_len_bytes) as usize;
        let mut state_hash_bytes = vec![0u8; hash_len];
        cursor.read_exact(&mut state_hash_bytes)?;
        let state_hash = String::from_utf8(state_hash_bytes)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Invalid UTF-8: {}", e)))?;
        
        let mut snap_hash_len_bytes = [0u8; 2];
        cursor.read_exact(&mut snap_hash_len_bytes)?;
        let snap_hash_len = u16::from_be_bytes(snap_hash_len_bytes) as usize;
        let mut snapshot_hash_bytes = vec![0u8; snap_hash_len];
        cursor.read_exact(&mut snapshot_hash_bytes)?;
        let snapshot_hash = String::from_utf8(snapshot_hash_bytes)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Invalid UTF-8: {}", e)))?;
        
        let mut timestamp_bytes = [0u8; 8];
        cursor.read_exact(&mut timestamp_bytes)?;
        let timestamp = i64::from_be_bytes(timestamp_bytes);
        
        let mut accounts_len_bytes = [0u8; 4];
        cursor.read_exact(&mut accounts_len_bytes)?;
        let accounts_len = u32::from_be_bytes(accounts_len_bytes) as usize;
        
        let mut active_validators_len_bytes = [0u8; 2];
        cursor.read_exact(&mut active_validators_len_bytes)?;
        let active_validators_len = u16::from_be_bytes(active_validators_len_bytes) as usize;
        
        let mut pending_validators_len_bytes = [0u8; 2];
        cursor.read_exact(&mut pending_validators_len_bytes)?;
        let pending_validators_len = u16::from_be_bytes(pending_validators_len_bytes) as usize;
        
        let mut reward_bytes = [0u8; 8];
        cursor.read_exact(&mut reward_bytes)?;
        let reward_pool_balance = i64::from_be_bytes(reward_bytes);
        
        let mut last_reward_bytes = [0u8; 8];
        cursor.read_exact(&mut last_reward_bytes)?;
        let last_reward_distribution = i64::from_be_bytes(last_reward_bytes);
        
        let mut proposals_len_bytes = [0u8; 2];
        cursor.read_exact(&mut proposals_len_bytes)?;
        let active_proposals_len = u16::from_be_bytes(proposals_len_bytes) as usize;
        
        let mut votes_len_bytes = [0u8; 4];
        cursor.read_exact(&mut votes_len_bytes)?;
        let proposal_votes_len = u32::from_be_bytes(votes_len_bytes) as usize;
        
        Ok((state_number, state_hash, snapshot_hash, timestamp, accounts_len, active_validators_len, pending_validators_len, reward_pool_balance, last_reward_distribution, active_proposals_len, proposal_votes_len))
    }

    /// Serialize a SnapshotRequest payload (requested state number)
    pub fn serialize_snapshot_request(state_number: i64) -> Result<Vec<u8>> {
        Ok(state_number.to_be_bytes().to_vec())
    }

    /// Deserialize a SnapshotRequest payload
    pub fn deserialize_snapshot_request(payload: &[u8]) -> Result<i64> {
        if payload.len() != 8 {
            return Err(crate::NetworkError::ProtocolError(
                "Invalid snapshot request payload".to_string(),
            ).into());
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(payload);
        Ok(i64::from_be_bytes(bytes))
    }

    /// Serialize a full Snapshot for SnapshotData message
    pub fn serialize_snapshot(snapshot: &Snapshot) -> Result<Vec<u8>> {
        // Use bincode for efficient serialization of complex structures
        bincode::serialize(snapshot)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Snapshot serialization failed: {}", e)).into())
    }

    /// Deserialize a full Snapshot from SnapshotData payload
    pub fn deserialize_snapshot(payload: &[u8]) -> Result<Snapshot> {
        bincode::deserialize(payload)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Snapshot deserialization failed: {}", e)).into())
    }

    /// Serialize a HandshakePayload for Handshake message
    pub fn serialize_handshake(payload: &crate::HandshakePayload) -> Result<Vec<u8>> {
        bincode::serialize(payload)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Handshake serialization failed: {}", e)).into())
    }

    /// Deserialize a HandshakePayload from Handshake message
    pub fn deserialize_handshake(payload: &[u8]) -> Result<crate::HandshakePayload> {
        bincode::deserialize(payload)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Handshake deserialization failed: {}", e)).into())
    }

    /// Serialize a SignedEnvelope (signed validator/committee payloads)
    pub fn serialize_signed_envelope(envelope: &crate::SignedEnvelope) -> Result<Vec<u8>> {
        bincode::serialize(envelope)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Envelope serialization failed: {}", e)).into())
    }

    /// Deserialize a SignedEnvelope from a message payload
    pub fn deserialize_signed_envelope(payload: &[u8]) -> Result<crate::SignedEnvelope> {
        bincode::deserialize(payload)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Envelope deserialization failed: {}", e)).into())
    }

    /// Serialize a list of SignedAnnouncements for PeerList gossip
    pub fn serialize_announcements(announcements: &[crate::SignedAnnouncement]) -> Result<Vec<u8>> {
        bincode::serialize(announcements)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Announcements serialization failed: {}", e)).into())
    }

    /// Deserialize SignedAnnouncements from a PeerList payload
    pub fn deserialize_announcements(payload: &[u8]) -> Result<Vec<crate::SignedAnnouncement>> {
        bincode::deserialize(payload)
            .map_err(|e| crate::NetworkError::ProtocolError(format!("Announcements deserialization failed: {}", e)).into())
    }
}

/// Message handler trait
pub trait MessageHandler {
    fn handle_message(&mut self, message: Message) -> Result<()>;
    fn can_handle(&self, message_type: MessageType) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let message = Message::new(
            MessageType::Handshake,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        assert_eq!(message.message_type, MessageType::Handshake);
        assert_eq!(message.sender, vec![1, 2, 3]);
        assert_eq!(message.payload, vec![4, 5, 6]);
    }

    #[test]
    fn test_message_serialization() {
        let message = Message::new(
            MessageType::Handshake,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let serialized = message.serialize();
        assert!(serialized.is_ok());
    }

    #[test]
    fn test_message_deserialization() {
        let message = Message::new(
            MessageType::Handshake,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let serialized = message.serialize().unwrap();
        let deserialized = Message::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.message_type, MessageType::Handshake);
        assert_eq!(deserialized.sender, vec![1, 2, 3]);
        assert_eq!(deserialized.payload, vec![4, 5, 6]);
    }
}
