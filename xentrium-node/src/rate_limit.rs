//! Connection/message rate limiting and temporary peer bans.
//!
//! Two layers:
//!  - accept-side: per-IP connection cap + handshake attempt window +
//!    strike-based temporary bans
//!  - receive-side: per-connection sliding-window quotas for the
//!    abuse-sensitive message types (PeerList, ValidatorRegistration,
//!    CommitteeSelection, SnapshotRequest)
//!
//! All thresholds come from NodeConfig so operators can tune them.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use xentrium_network::MessageType;

/// Process-wide abuse counters, rendered by the /metrics endpoint.
#[derive(Default)]
pub struct AbuseCounters {
    pub rate_limit_hits: AtomicU64,
    pub rejected_connections: AtomicU64,
    pub rejected_handshakes: AtomicU64,
    pub replay_rejections: AtomicU64,
    pub peer_bans: AtomicU64,
}

impl AbuseCounters {
    pub fn bump(&self, c: &AtomicU64) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

pub enum AcceptVerdict {
    Allow,
    /// Over per-IP connection cap or handshake rate window.
    RateLimited,
    /// Currently banned — drop immediately.
    Banned,
}

/// Accept-side limiter: tracks handshake attempts + strikes + bans per IP.
pub struct RateLimiter {
    conn_attempts: HashMap<IpAddr, VecDeque<Instant>>,
    strikes: HashMap<IpAddr, u32>,
    banned_until: HashMap<IpAddr, Instant>,
    max_conns_per_ip: usize,
    handshake_burst: u32,
    handshake_window: Duration,
    ban_duration: Duration,
    ban_strike_threshold: u32,
}

pub struct RateLimitConfig {
    pub max_conns_per_ip: usize,
    pub handshake_rate_per_min: u32,
    pub ban_duration_secs: u64,
    pub ban_strike_threshold: u32,
}

impl RateLimiter {
    pub fn new(cfg: &RateLimitConfig) -> Self {
        Self {
            conn_attempts: HashMap::new(),
            strikes: HashMap::new(),
            banned_until: HashMap::new(),
            max_conns_per_ip: cfg.max_conns_per_ip.max(1),
            handshake_burst: cfg.handshake_rate_per_min.max(1),
            handshake_window: Duration::from_secs(60),
            ban_duration: Duration::from_secs(cfg.ban_duration_secs.max(30)),
            ban_strike_threshold: cfg.ban_strike_threshold.max(1),
        }
    }

    pub fn is_banned(&mut self, ip: &IpAddr) -> bool {
        match self.banned_until.get(ip) {
            Some(until) if Instant::now() < *until => true,
            Some(_) => {
                self.banned_until.remove(ip);
                self.strikes.remove(ip);
                false
            }
            None => false,
        }
    }

    /// Check an inbound TCP connection. `conns_from_ip` is the number of
    /// already-established connections from this IP.
    pub fn check_inbound(&mut self, ip: IpAddr, conns_from_ip: usize) -> AcceptVerdict {
        if self.is_banned(&ip) {
            return AcceptVerdict::Banned;
        }
        if conns_from_ip >= self.max_conns_per_ip {
            return AcceptVerdict::RateLimited;
        }
        // Sliding-window handshake attempt accounting
        let now = Instant::now();
        let attempts = self.conn_attempts.entry(ip).or_insert_with(VecDeque::new);
        while attempts.front().map(|t| now.duration_since(*t) > self.handshake_window).unwrap_or(false) {
            attempts.pop_front();
        }
        if attempts.len() >= self.handshake_burst as usize {
            return AcceptVerdict::RateLimited;
        }
        attempts.push_back(now);
        AcceptVerdict::Allow
    }

    /// Record an abuse strike against an IP; returns true when this
    /// strike triggered a fresh ban.
    pub fn record_strike(&mut self, ip: IpAddr) -> bool {
        if self.is_banned(&ip) {
            return false;
        }
        let n = self.strikes.entry(ip).or_insert(0);
        *n += 1;
        if *n >= self.ban_strike_threshold {
            self.ban(ip);
            return true;
        }
        false
    }

    pub fn ban(&mut self, ip: IpAddr) {
        self.banned_until.insert(ip, Instant::now() + self.ban_duration);
    }

    pub fn banned_count(&self) -> usize {
        self.banned_until.len()
    }

    /// Drop bookkeeping for IPs that haven't attempted recently.
    pub fn prune(&mut self) {
        let now = Instant::now();
        self.banned_until.retain(|_, until| now < *until);
        self.conn_attempts.retain(|_, dq| {
            while dq.front().map(|t| now.duration_since(*t) > Duration::from_secs(600)).unwrap_or(false) {
                dq.pop_front();
            }
            !dq.is_empty()
        });
        self.strikes.retain(|ip, _| self.banned_until.contains_key(ip));
    }
}

/// Receive-side per-connection quota for abuse-sensitive message types.
pub struct PerConnQuota {
    buckets: HashMap<MessageType, VecDeque<Instant>>,
    /// Global per-type ceiling (ValidatorRegistration and fallback).
    global_per_min: u32,
}

impl PerConnQuota {
    pub fn new(global_per_min: u32) -> Self {
        Self {
            buckets: HashMap::new(),
            global_per_min: global_per_min.max(10),
        }
    }

    fn limit_for(&self, t: &MessageType) -> u32 {
        // Stricter static caps for types whose legitimate rate is low.
        // ValidatorRegistration is intentionally the global ceiling:
        // legit volume scales with validator count (~2 msgs/validator/cycle).
        match t {
            MessageType::PeerList => 60,
            MessageType::CommitteeSelection => 120,
            MessageType::SnapshotRequest => 240,
            _ => self.global_per_min,
        }
    }

    fn protected(t: &MessageType) -> bool {
        matches!(
            t,
            MessageType::PeerList
                | MessageType::ValidatorRegistration
                | MessageType::CommitteeSelection
                | MessageType::SnapshotRequest
        )
    }

    /// Returns false when the message exceeds its type quota.
    pub fn allow(&mut self, t: &MessageType) -> bool {
        if !Self::protected(t) {
            return true;
        }
        let limit = self.limit_for(t) as usize;
        let now = Instant::now();
        let dq = self.buckets.entry(t.clone()).or_insert_with(VecDeque::new);
        while dq.front().map(|i| now.duration_since(*i) > Duration::from_secs(60)).unwrap_or(false) {
            dq.pop_front();
        }
        if dq.len() >= limit {
            return false;
        }
        dq.push_back(now);
        true
    }
}
