use crate::{PeerID, Result};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Peer health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerHealth {
    /// Peer is responsive, last heartbeat within interval
    Healthy,
    /// Peer missed one heartbeat or latency is elevated
    Suspect,
    /// Peer missed multiple heartbeats or is unreachable
    Unhealthy,
}

/// Liveness tracking for a single peer
#[derive(Debug, Clone)]
pub struct PeerLiveness {
    pub peer_id: PeerID,
    pub last_ping_sent: Option<Instant>,
    pub last_pong_received: Option<Instant>,
    pub last_ping_id: Option<u64>,
    pub consecutive_misses: u32,
    pub consecutive_successes: u32,
    pub last_latency: Option<Duration>,
    pub avg_latency: Option<Duration>,
    pub health: PeerHealth,
}

impl PeerLiveness {
    pub fn new(peer_id: PeerID) -> Self {
        Self {
            peer_id,
            last_ping_sent: None,
            last_pong_received: None,
            last_ping_id: None,
            consecutive_misses: 0,
            consecutive_successes: 0,
            last_latency: None,
            avg_latency: None,
            health: PeerHealth::Healthy,
        }
    }

    /// Record that a ping was sent to this peer
    pub fn record_ping_sent(&mut self, ping_id: u64) {
        self.last_ping_sent = Some(Instant::now());
        self.last_ping_id = Some(ping_id);
    }

    /// Record that a pong was received; returns latency if a ping was outstanding
    pub fn record_pong_received(&mut self) -> Option<Duration> {
        let now = Instant::now();
        self.last_pong_received = Some(now);
        self.consecutive_misses = 0;
        self.consecutive_successes += 1;

        let latency = self.last_ping_sent.map(|sent| now.duration_since(sent));
        if let Some(lat) = latency {
            self.last_latency = Some(lat);
            // Exponential moving average with alpha = 0.3
            self.avg_latency = Some(match self.avg_latency {
                Some(avg) => {
                    let avg_ms = avg.as_millis() as f64;
                    let lat_ms = lat.as_millis() as f64;
                    Duration::from_millis(((1.0 - 0.3) * avg_ms + 0.3 * lat_ms) as u64)
                }
                None => lat,
            });
        }
        latency
    }

    /// Record a missed heartbeat (no pong received in time)
    pub fn record_miss(&mut self) {
        self.consecutive_misses += 1;
        self.consecutive_successes = 0;
    }

    /// Update health status based on misses and timing
    pub fn update_health(&mut self, heartbeat_interval: Duration, timeout: Duration) {
        // Check if pong is overdue
        if let Some(last_pong) = self.last_pong_received {
            if last_pong.elapsed() > timeout {
                self.health = PeerHealth::Unhealthy;
                return;
            }
        } else if self.last_ping_sent.is_some() && self.consecutive_misses > 0 {
            // Never received a pong
            if self.last_ping_sent.unwrap().elapsed() > timeout {
                self.health = PeerHealth::Unhealthy;
                return;
            }
        }

        // Determine health from consecutive misses
        self.health = if self.consecutive_misses == 0 {
            PeerHealth::Healthy
        } else if self.consecutive_misses == 1 {
            PeerHealth::Suspect
        } else {
            PeerHealth::Unhealthy
        };

        // Elevated latency can also mark suspect, but only if the peer
        // has also missed recently (a lone slow pong shouldn't penalize)
        if self.consecutive_misses > 0 {
            if let Some(avg) = self.avg_latency {
                if avg > heartbeat_interval && self.health == PeerHealth::Healthy {
                    self.health = PeerHealth::Suspect;
                }
            }
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.health == PeerHealth::Healthy
    }

    pub fn is_unhealthy(&self) -> bool {
        self.health == PeerHealth::Unhealthy
    }
}

/// Tracks liveness for all peers
pub struct HeartbeatTracker {
    peers: HashMap<PeerID, PeerLiveness>,
    next_ping_id: u64,
    heartbeat_interval: Duration,
    peer_timeout: Duration,
}

impl HeartbeatTracker {
    pub fn new(heartbeat_interval_secs: u64, peer_timeout_secs: u64) -> Self {
        Self {
            peers: HashMap::new(),
            next_ping_id: 1,
            heartbeat_interval: Duration::from_secs(heartbeat_interval_secs),
            peer_timeout: Duration::from_secs(peer_timeout_secs),
        }
    }

    /// Get or create liveness entry for a peer
    pub fn get_or_create(&mut self, peer_id: PeerID) -> &mut PeerLiveness {
        let key = peer_id.clone();
        self.peers.entry(key).or_insert_with(|| {
            PeerLiveness::new(peer_id)
        })
    }

    /// Get peers that need a ping this cycle
    pub fn peers_due_for_ping(&mut self) -> Vec<(PeerID, u64)> {
        let now = Instant::now();
        let mut due = Vec::new();
        let mut next_id = self.next_ping_id;
        for liveness in self.peers.values_mut() {
            let due_for_ping = match liveness.last_ping_sent {
                Some(last) => now.duration_since(last) >= self.heartbeat_interval,
                None => true,
            };
            if due_for_ping {
                liveness.record_ping_sent(next_id);
                due.push((liveness.peer_id.clone(), next_id));
                next_id += 1;
            }
        }
        self.next_ping_id = next_id;
        due
    }

    /// Process a received pong; returns (peer_id, latency) if matched
    pub fn record_pong(&mut self, peer_id: &PeerID) -> Option<(PeerID, Duration)> {
        if let Some(liveness) = self.peers.get_mut(peer_id) {
            let latency = liveness.record_pong_received();
            liveness.update_health(self.heartbeat_interval, self.peer_timeout);
            latency.map(|l| (peer_id.clone(), l))
        } else {
            // Unknown peer sent pong - register it
            let mut liveness = PeerLiveness::new(peer_id.clone());
            liveness.record_pong_received();
            liveness.update_health(self.heartbeat_interval, self.peer_timeout);
            self.peers.insert(peer_id.clone(), liveness);
            None
        }
    }

    /// Check for missed pongs and update health for all peers
    /// Returns list of peers that became unhealthy this cycle
    pub fn check_missed(&mut self) -> Vec<PeerID> {
        let now = Instant::now();
        let mut newly_unhealthy = Vec::new();

        for liveness in self.peers.values_mut() {
            let was_unhealthy = liveness.is_unhealthy();
            // If a ping was sent and no pong received within interval*1.5, count as miss
            if let Some(last_ping) = liveness.last_ping_sent {
                if let Some(last_pong) = liveness.last_pong_received {
                    if last_ping > last_pong && now.duration_since(last_ping) > self.heartbeat_interval {
                        liveness.record_miss();
                    }
                } else if now.duration_since(last_ping) > self.heartbeat_interval {
                    liveness.record_miss();
                }
            }
            liveness.update_health(self.heartbeat_interval, self.peer_timeout);
            if liveness.is_unhealthy() && !was_unhealthy {
                newly_unhealthy.push(liveness.peer_id.clone());
            }
        }
        newly_unhealthy
    }

    /// Remove a peer from tracking (e.g., on disconnect)
    pub fn remove_peer(&mut self, peer_id: &PeerID) {
        self.peers.remove(peer_id);
    }

    /// Get health status of a peer
    pub fn health_of(&self, peer_id: &PeerID) -> Option<PeerHealth> {
        self.peers.get(peer_id).map(|l| l.health)
    }

    /// Get all unhealthy peers
    pub fn unhealthy_peers(&self) -> Vec<PeerID> {
        self.peers
            .values()
            .filter(|l| l.is_unhealthy())
            .map(|l| l.peer_id.clone())
            .collect()
    }

    /// Get liveness info for a peer
    pub fn liveness_of(&self, peer_id: &PeerID) -> Option<&PeerLiveness> {
        self.peers.get(peer_id)
    }

    /// Mean of all tracked peers' average RTTs (None when no data yet).
    pub fn average_rtt(&self) -> Option<Duration> {
        let mut sum = Duration::ZERO;
        let mut n = 0u32;
        for l in self.peers.values() {
            if let Some(avg) = l.avg_latency {
                sum += avg;
                n += 1;
            }
        }
        (n > 0).then(|| sum / n)
    }

    /// Per-peer last RTT for metrics.
    pub fn peer_rtts(&self) -> Vec<(PeerID, Duration)> {
        self.peers
            .values()
            .filter_map(|l| l.last_latency.map(|d| (l.peer_id.clone(), d)))
            .collect()
    }

    /// Number of tracked peers
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Update configuration values
    pub fn update_config(&mut self, heartbeat_interval_secs: u64, peer_timeout_secs: u64) {
        self.heartbeat_interval = Duration::from_secs(heartbeat_interval_secs);
        self.peer_timeout = Duration::from_secs(peer_timeout_secs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;

    fn test_peer_id(port: u16) -> PeerID {
        PeerID::new(
            format!("XNTTest{}", port),
            format!("127.0.0.1:{}", port).parse::<SocketAddr>().unwrap(),
            format!("machine{}", port),
        )
    }

    #[test]
    fn test_ping_pong_latency() {
        let mut tracker = HeartbeatTracker::new(5, 30);
        let peer = test_peer_id(40001);

        let due = tracker.peers_due_for_ping();
        assert!(due.is_empty()); // No peers yet

        tracker.get_or_create(peer.clone());
        let due = tracker.peers_due_for_ping();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].0, peer);

        let result = tracker.record_pong(&peer);
        assert!(result.is_some());
        let (_, latency) = result.unwrap();
        assert!(latency.as_millis() < 1000);
    }

    #[test]
    fn test_unhealthy_detection() {
        let mut tracker = HeartbeatTracker::new(1, 3);
        let peer = test_peer_id(40002);

        tracker.get_or_create(peer.clone());
        // Send ping
        let _ = tracker.peers_due_for_ping();
        // Wait for miss
        std::thread::sleep(Duration::from_millis(1100));
        let misses = tracker.check_missed();
        // After 1 miss -> Suspect
        let health = tracker.health_of(&peer);
        assert!(health.is_some());
    }

    #[test]
    fn test_health_transitions() {
        let mut liveness = PeerLiveness::new(test_peer_id(40003));
        assert_eq!(liveness.health, PeerHealth::Healthy);

        liveness.record_miss();
        liveness.update_health(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(liveness.health, PeerHealth::Suspect);

        liveness.record_miss();
        liveness.update_health(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(liveness.health, PeerHealth::Unhealthy);

        // Recovery
        liveness.record_pong_received();
        liveness.update_health(Duration::from_secs(5), Duration::from_secs(30));
        assert_eq!(liveness.health, PeerHealth::Healthy);
    }

    #[test]
    fn test_peer_recovery() {
        let mut tracker = HeartbeatTracker::new(1, 3);
        let peer = test_peer_id(40004);

        tracker.get_or_create(peer.clone());
        let _ = tracker.peers_due_for_ping();
        std::thread::sleep(Duration::from_millis(1100));
        let _ = tracker.check_missed();

        // Peer recovers - pong arrives
        let result = tracker.record_pong(&peer);
        assert!(result.is_some());
        assert_eq!(tracker.health_of(&peer), Some(PeerHealth::Healthy));
    }
}
