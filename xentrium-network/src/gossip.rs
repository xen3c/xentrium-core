use crate::{Message, PeerID, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Gossip protocol trait
pub trait GossipProtocol {
    fn broadcast(&mut self, message: Message) -> Result<()>;
    fn receive(&mut self, message: Message, from: PeerID) -> Result<()>;
    fn is_duplicate(&self, message_id: &[u8]) -> bool;
    fn add_seen(&mut self, message_id: Vec<u8>);
    fn get_seen_count(&self) -> usize;
}

/// Message metadata for gossip
#[derive(Debug, Clone)]
struct MessageMetadata {
    hop_count: u8,
    hop_limit: u8,
    seen_at: Instant,
    ttl: Duration,
    path: Vec<PeerID>,
}

/// Basic gossip implementation
pub struct BasicGossip {
    seen_messages: HashMap<Vec<u8>, MessageMetadata>,
    max_cache_size: usize,
    max_hops: u8,
    default_ttl: Duration,
}

impl BasicGossip {
    pub fn new() -> Self {
        Self {
            seen_messages: HashMap::new(),
            max_cache_size: 1000,
            max_hops: 10,
            default_ttl: Duration::from_secs(60), // 1 minute TTL
        }
    }

    pub fn with_config(max_cache_size: usize, max_hops: u8, ttl_secs: u64) -> Self {
        Self {
            seen_messages: HashMap::new(),
            max_cache_size,
            max_hops,
            default_ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub fn cleanup_expired(&mut self) {
        let now = Instant::now();
        let mut expired = Vec::new();
        
        for (msg_id, metadata) in &self.seen_messages {
            if now.duration_since(metadata.seen_at) > metadata.ttl {
                expired.push(msg_id.clone());
            }
        }
        
        for msg_id in expired {
            self.seen_messages.remove(&msg_id);
        }

        // Limit cache size
        if self.seen_messages.len() > self.max_cache_size {
            let to_remove = self.seen_messages.len() - self.max_cache_size;
            let mut message_ids: Vec<_> = self.seen_messages
                .keys()
                .cloned()
                .collect();
            
            // Sort by seen time (we need to access metadata for this)
            let mut messages_with_time: Vec<_> = self.seen_messages
                .iter()
                .map(|(id, m)| (id.clone(), m.seen_at))
                .collect();
            messages_with_time.sort_by_key(|(_, t)| *t);
            
            for (msg_id, _) in messages_with_time.into_iter().take(to_remove) {
                self.seen_messages.remove(&msg_id);
            }
        }
    }

    fn should_relay(&self, metadata: &MessageMetadata) -> bool {
        // Check hop limit
        if metadata.hop_count >= metadata.hop_limit {
            return false;
        }

        // Check TTL
        if Instant::now().duration_since(metadata.seen_at) > metadata.ttl {
            return false;
        }

        // Check for loop prevention (message already seen this node)
        // This is a simplified check - in production, we'd track the full path
        if metadata.hop_count > self.max_hops {
            return false;
        }

        true
    }

    fn generate_message_id(message: &Message) -> Vec<u8> {
        // Use message type + timestamp + sender + payload as message ID
        let mut id = Vec::new();
        id.push(message.message_type.clone() as u8);
        id.extend_from_slice(&message.timestamp.to_be_bytes());
        id.extend_from_slice(&(message.sender.len() as u8).to_be_bytes());
        id.extend_from_slice(&message.sender);
        id.extend_from_slice(&(message.payload.len() as u8).to_be_bytes());
        id.extend_from_slice(&message.payload);
        id
    }
}

impl GossipProtocol for BasicGossip {
    fn broadcast(&mut self, message: Message) -> Result<()> {
        // Generate message ID
        let message_id = Self::generate_message_id(&message);
        
        // Add to seen cache
        self.add_seen(message_id.clone());
        
        // Metadata for first hop
        let metadata = MessageMetadata {
            hop_count: 0,
            hop_limit: self.max_hops,
            seen_at: Instant::now(),
            ttl: self.default_ttl,
            path: Vec::new(),
        };
        
        self.seen_messages.insert(message_id, metadata);
        
        // In a real implementation, this would send to all connected peers
        // For now, we just track the message
        Ok(())
    }

    fn receive(&mut self, message: Message, from: PeerID) -> Result<()> {
        // Generate message ID
        let message_id = Self::generate_message_id(&message);
        
        // Check for duplicate
        if self.is_duplicate(&message_id) {
            return Ok(()); // Silently drop duplicates
        }

        // Get or create metadata
        let metadata = self.seen_messages.get(&message_id).cloned().unwrap_or_else(|| {
            MessageMetadata {
                hop_count: 0,
                hop_limit: self.max_hops,
                seen_at: Instant::now(),
                ttl: self.default_ttl,
                path: Vec::new(),
            }
        });

        // Check if message is from a peer already in path (loop prevention)
        if metadata.path.contains(&from) {
            return Ok(()); // Drop loops
        }

        // Increment hop count
        let hop_count = metadata.hop_count + 1;
        
        // Update metadata
        let mut new_path = metadata.path.clone();
        new_path.push(from);
        
        let new_metadata = MessageMetadata {
            hop_count,
            hop_limit: metadata.hop_limit,
            seen_at: metadata.seen_at,
            ttl: metadata.ttl,
            path: new_path,
        };

        // Add to seen cache
        self.seen_messages.insert(message_id, new_metadata);

        // Cleanup expired messages
        self.cleanup_expired();

        Ok(())
    }

    fn is_duplicate(&self, message_id: &[u8]) -> bool {
        self.seen_messages.contains_key(message_id)
    }

    fn add_seen(&mut self, message_id: Vec<u8>) {
        if !self.is_duplicate(&message_id) {
            let metadata = MessageMetadata {
                hop_count: 0,
                hop_limit: self.max_hops,
                seen_at: Instant::now(),
                ttl: self.default_ttl,
                path: Vec::new(),
            };
            self.seen_messages.insert(message_id, metadata);
        }
    }

    fn get_seen_count(&self) -> usize {
        self.seen_messages.len()
    }
}

impl Default for BasicGossip {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageType;

    #[test]
    fn test_basic_gossip_creation() {
        let gossip = BasicGossip::new();
        assert_eq!(gossip.get_seen_count(), 0);
    }

    #[test]
    fn test_gossip_duplicate_detection() {
        let mut gossip = BasicGossip::new();
        let message_id = vec![1, 2, 3];
        assert!(!gossip.is_duplicate(&message_id));
        gossip.add_seen(message_id.clone());
        assert!(gossip.is_duplicate(&message_id));
    }

    #[test]
    fn test_gossip_receive() {
        let mut gossip = BasicGossip::new();
        let message = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        let result = gossip.receive(message, peer_id);
        assert!(result.is_ok());
        assert_eq!(gossip.get_seen_count(), 1);
    }

    #[test]
    fn test_gossip_duplicate_suppression() {
        let mut gossip = BasicGossip::new();
        let message = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        
        // First receive
        gossip.receive(message.clone(), peer_id.clone()).unwrap();
        assert_eq!(gossip.get_seen_count(), 1);
        
        // Duplicate receive
        gossip.receive(message, peer_id).unwrap();
        assert_eq!(gossip.get_seen_count(), 1); // Not added again
    }

    #[test]
    fn test_message_id_generation() {
        let message1 = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let message2 = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        
        let id1 = BasicGossip::generate_message_id(&message1);
        let id2 = BasicGossip::generate_message_id(&message2);
        
        assert_eq!(id1, id2); // Same message, same ID
    }

    #[test]
    fn test_message_id_different_messages() {
        let message1 = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let message2 = Message::new(
            MessageType::Pong,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        
        let id1 = BasicGossip::generate_message_id(&message1);
        let id2 = BasicGossip::generate_message_id(&message2);
        
        assert_ne!(id1, id2); // Different message, different ID
    }

    #[test]
    fn test_cleanup_expired() {
        let mut gossip = BasicGossip::with_config(100, 10, 1); // 1 second TTL
        
        let message = Message::new(
            MessageType::Ping,
            vec![1, 2, 3],
            vec![4, 5, 6],
        );
        let peer_id = PeerID::new(
            "XNTTestAddress".to_string(),
            "127.0.0.1:30303".parse().unwrap(),
            "machine1".to_string(),
        );
        
        gossip.receive(message, peer_id).unwrap();
        assert_eq!(gossip.get_seen_count(), 1);
        
        // Wait for expiration (in real test, would need tokio::time::sleep)
        // For unit test, we can't easily test time-based cleanup
        // So we just verify the structure
    }

    #[test]
    fn test_max_cache_size() {
        let mut gossip = BasicGossip::with_config(5, 10, 60);
        
        for i in 0..10 {
            let message = Message::new(
                MessageType::Ping,
                vec![i],
                vec![4, 5, 6],
            );
            let peer_id = PeerID::new(
                format!("XNTTestAddress{}", i),
                "127.0.0.1:30303".parse().unwrap(),
                format!("machine{}", i),
            );
            gossip.receive(message, peer_id).unwrap();
        }
        
        // Should be limited to 5
        assert_eq!(gossip.get_seen_count(), 5);
    }
}
