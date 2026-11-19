use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use xentrium_shared::{
    Address, Balance, ValidatorInfo, ValidatorState, GovernanceProposal, 
    GovernanceStatus, Result, XentriumError,
    MAX_COMMITTEE_SIZE, MAX_CANDIDATE_POOL, CONSENSUS_THRESHOLD
};
use rand::seq::SliceRandom;
use rand::Rng;

/// Committee information
#[derive(Debug, Clone)]
pub struct CommitteeInfo {
    pub members: Vec<Address>,
    pub selection_time: i64,
    pub state_number: i64,
    pub threshold: f64,
}

/// Vote counter for governance proposals
#[derive(Debug, Clone)]
pub struct VoteCounter {
    proposal_id: u64,
    approve_votes: u64,
    reject_votes: u64,
    votes: HashMap<Address, bool>,
    committee_weights: HashMap<Address, f64>,
}

impl VoteCounter {
    pub fn new(proposal_id: u64) -> Self {
        Self {
            proposal_id,
            approve_votes: 0,
            reject_votes: 0,
            votes: HashMap::new(),
            committee_weights: HashMap::new(),
        }
    }

    pub fn add_vote(&mut self, voter: Address, vote: bool, weight: f64) {
        self.votes.insert(voter.clone(), vote);
        self.committee_weights.insert(voter, weight);
        if vote {
            self.approve_votes += weight as u64;
        } else {
            self.reject_votes += weight as u64;
        }
    }

    pub fn get_result(&self) -> GovernanceStatus {
        let total = self.approve_votes + self.reject_votes;
        if total == 0 {
            return GovernanceStatus::Proposed;
        }

        let approve_ratio = self.approve_votes as f64 / total as f64;
        let reject_ratio = self.reject_votes as f64 / total as f64;

        if approve_ratio >= CONSENSUS_THRESHOLD {
            GovernanceStatus::Approved
        } else if reject_ratio >= CONSENSUS_THRESHOLD {
            GovernanceStatus::Rejected
        } else {
            GovernanceStatus::Voting
        }
    }

    pub fn proposal_id(&self) -> u64 {
        self.proposal_id
    }

    pub fn approve_count(&self) -> u64 {
        self.approve_votes
    }

    pub fn reject_count(&self) -> u64 {
        self.reject_votes
    }
}

/// Validator manager for committee selection and validator lifecycle
pub struct ValidatorManager {
    active_validators: HashMap<Address, ValidatorInfo>,
    pending_validators: HashMap<Address, ValidatorInfo>,
    current_committee: Option<CommitteeInfo>,
    vote_counter: Option<VoteCounter>,
    state_number: i64,
}

impl ValidatorManager {
    pub fn new() -> Self {
        Self {
            active_validators: HashMap::new(),
            pending_validators: HashMap::new(),
            current_committee: None,
            vote_counter: None,
            state_number: 0,
        }
    }

    /// Get current state number
    pub fn state_number(&self) -> i64 {
        self.state_number
    }

    /// Set state number (called by state engine)
    pub fn set_state_number(&mut self, state_number: i64) {
        self.state_number = state_number;
    }

    /// Register a new validator (moves to pending)
    pub fn register_validator(&mut self, address: Address, public_key: Vec<u8>, machine_id: String) -> Result<()> {
        if self.active_validators.contains_key(&address) || self.pending_validators.contains_key(&address) {
            return Err(XentriumError::ValidatorError("Validator already registered".to_string()));
        }

        let validator = ValidatorInfo {
            address: address.clone(),
            public_key,
            machine_id,
            balance: 0,
            state: ValidatorState::Pending,
            enabled_at: None,
            uptime_score: 0,
            reputation_score: 0,
            validator_weight: 0.0,
            last_seen: current_timestamp(),
            consecutive_uptime_weeks: 0,
            committees_participated: 0,
            votes_cast: 0,
            votes_correct: 0,
            endpoint: String::new(),
            is_active: false,
        };

        self.pending_validators.insert(address, validator);
        Ok(())
    }

    /// Register a new validator from network (uses ValidatorInfo)
    pub fn register_validator_from_network(&mut self, validator: ValidatorInfo) -> Result<()> {
        if self.active_validators.contains_key(&validator.address) || self.pending_validators.contains_key(&validator.address) {
            return Ok(()); // Already registered, ignore
        }

        let is_active = validator.state == ValidatorState::Active;

        let full_validator = ValidatorInfo {
            address: validator.address.clone(),
            public_key: vec![],
            machine_id: String::new(),
            balance: validator.balance,
            state: validator.state,
            enabled_at: None,
            uptime_score: validator.uptime_score,
            reputation_score: validator.reputation_score,
            validator_weight: 0.0,
            last_seen: current_timestamp(),
            consecutive_uptime_weeks: 0,
            committees_participated: 0,
            votes_cast: 0,
            votes_correct: 0,
            endpoint: String::new(),
            is_active,
        };

        if is_active {
            self.active_validators.insert(validator.address.clone(), full_validator);
        } else {
            self.pending_validators.insert(validator.address.clone(), full_validator);
        }
        Ok(())
    }

    /// Update committee from network
    pub fn update_committee(&mut self, members: &[Address], state_number: i64) {
        self.state_number = state_number;
        self.current_committee = Some(CommitteeInfo {
            members: members.to_vec(),
            selection_time: current_timestamp(),
            state_number,
            threshold: CONSENSUS_THRESHOLD,
        });
    }

    /// Get current committee members
    pub fn get_committee_members(&self) -> Vec<Address> {
        if let Some(ref committee) = self.current_committee {
            committee.members.clone()
        } else {
            vec![]
        }
    }

    /// Enable a validator (moves from pending to active)
    pub fn enable_validator(&mut self, address: Address) -> Result<()> {
        if let Some(mut validator) = self.pending_validators.remove(&address) {
            validator.state = ValidatorState::Active;
            validator.enabled_at = Some(current_timestamp());
            validator.is_active = true;
            self.active_validators.insert(address, validator);
            Ok(())
        } else {
            Err(XentriumError::ValidatorError("Validator not in pending state".to_string()))
        }
    }

    /// Disable a validator (moves from active to suspended)
    pub fn disable_validator(&mut self, address: Address) -> Result<()> {
        if let Some(mut validator) = self.active_validators.remove(&address) {
            validator.state = ValidatorState::Suspended { until: current_timestamp() + 86400 * 30 }; // 30 days
            validator.is_active = false;
            self.active_validators.insert(address, validator);
            Ok(())
        } else {
            Err(XentriumError::ValidatorError("Validator not active".to_string()))
        }
    }

    /// Update validator uptime score
    pub fn update_uptime(&mut self, address: Address, score: i32) -> Result<()> {
        if let Some(validator) = self.active_validators.get_mut(&address) {
            validator.uptime_score = score;
            validator.last_seen = current_timestamp();
            Ok(())
        } else {
            Err(XentriumError::ValidatorError("Validator not found".to_string()))
        }
    }

    /// Update validator balance and recalculate reputation
    pub fn update_balance(&mut self, address: Address, balance: Balance) -> Result<()> {
        if let Some(validator) = self.active_validators.get_mut(&address) {
            validator.balance = balance;
            let balance_score = balance / 100_000_000; // Convert to XNT units
            validator.reputation_score = balance_score + (validator.uptime_score as i64) * 100;
            validator.validator_weight = if validator.reputation_score > 0 {
                (validator.reputation_score as f64).sqrt()
            } else {
                0.0
            };
            Ok(())
        } else {
            Err(XentriumError::ValidatorError("Validator not found".to_string()))
        }
    }

    /// Get active validators count
    pub fn active_validator_count(&self) -> usize {
        self.active_validators.len()
    }

    /// Get all active validators
    pub fn get_active_validators(&self) -> Vec<ValidatorInfo> {
        self.active_validators.values().cloned().collect()
    }

    /// Get all pending validators
    pub fn get_pending_validators(&self) -> Vec<ValidatorInfo> {
        self.pending_validators.values().cloned().collect()
    }

    /// Check if validator is in committee
    pub fn is_committee_member(&self, address: &Address) -> bool {
        if let Some(ref committee) = self.current_committee {
            committee.members.contains(address)
        } else {
            false
        }
    }

    /// Get current committee
    pub fn get_current_committee(&self) -> Option<&CommitteeInfo> {
        self.current_committee.as_ref()
    }

    /// Select committee from active validators
    pub fn select_committee(&mut self) -> Result<CommitteeInfo> {
        let active_count = self.active_validators.len();
        
        // Dynamic committee sizing
        let committee_size = std::cmp::min(MAX_COMMITTEE_SIZE, active_count);
        let pool_size = std::cmp::min(MAX_CANDIDATE_POOL, active_count);

        if committee_size == 0 {
            return Err(XentriumError::ValidatorError("No active validators".to_string()));
        }

        // Build candidate pool (uptime_score >= 80)
        let mut candidates: Vec<_> = self.active_validators
            .values()
            .filter(|v| v.uptime_score >= 80)
            .cloned()
            .collect();

        // Sort by reputation_score (descending)
        candidates.sort_by(|a, b| b.reputation_score.cmp(&a.reputation_score));

        // Take top pool_size
        candidates.truncate(pool_size);

        if candidates.is_empty() {
            return Err(XentriumError::ValidatorError("No eligible validators (uptime < 80)".to_string()));
        }

        // Calculate weights and apply cap (5%)
        let total_weight: f64 = candidates.iter().map(|v| v.validator_weight).sum();
        let max_weight = total_weight * 0.05;
        
        let mut weights: Vec<f64> = candidates
            .iter()
            .map(|v| v.validator_weight.min(max_weight))
            .collect();

        // Weighted random selection
        let mut rng = rand::thread_rng();
        let mut committee: Vec<Address> = Vec::new();
        let mut selected_indices = Vec::new();

        for _ in 0..committee_size {
            if weights.is_empty() {
                break;
            }

            let weight_sum: f64 = weights.iter().sum();
            if weight_sum <= 0.0 {
                // Fallback to random selection if all weights are 0
                let remaining: Vec<_> = (0..candidates.len())
                    .filter(|i| !selected_indices.contains(i))
                    .collect();
                if let Some(&idx) = remaining.choose(&mut rng) {
                    committee.push(candidates[idx].address.clone());
                    selected_indices.push(idx);
                }
            } else {
                let mut rnd = rng.gen::<f64>() * weight_sum;
                let mut selected_idx = 0;
                for (idx, &w) in weights.iter().enumerate() {
                    if selected_indices.contains(&idx) {
                        continue;
                    }
                    if rnd < w {
                        selected_idx = idx;
                        break;
                    }
                    rnd -= w;
                }
                
                committee.push(candidates[selected_idx].address.clone());
                selected_indices.push(selected_idx);
            }
        }

        let committee_info = CommitteeInfo {
            members: committee,
            selection_time: current_timestamp(),
            state_number: self.state_number,
            threshold: CONSENSUS_THRESHOLD,
        };

        self.current_committee = Some(committee_info.clone());
        Ok(committee_info)
    }

    /// Check if committee should rotate (every 24 hours)
    pub fn should_rotate_committee(&self) -> bool {
        if let Some(ref committee) = self.current_committee {
            let elapsed = current_timestamp() - committee.selection_time;
            elapsed >= 86400 // 24 hours in seconds
        } else {
            true // No committee exists, should create one
        }
    }

    /// Start voting on a proposal
    pub fn start_voting(&mut self, proposal: GovernanceProposal) {
        self.vote_counter = Some(VoteCounter::new(proposal.proposal_id));
    }

    /// Add a vote to the current proposal
    pub fn add_vote(&mut self, vote: bool, voter: Address, weight: f64) -> Result<()> {
        if let Some(ref mut counter) = self.vote_counter {
            counter.add_vote(voter, vote, weight);
            Ok(())
        } else {
            Err(XentriumError::ValidatorError("No active voting session".to_string()))
        }
    }

    /// Get current vote result
    pub fn get_vote_result(&self) -> GovernanceStatus {
        if let Some(ref counter) = self.vote_counter {
            counter.get_result()
        } else {
            GovernanceStatus::Proposed
        }
    }

    /// Get current vote counter
    pub fn get_vote_counter(&self) -> Option<&VoteCounter> {
        self.vote_counter.as_ref()
    }
}

/// Helper to get current timestamp
fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_manager_creation() {
        let manager = ValidatorManager::new();
        assert_eq!(manager.active_validator_count(), 0);
        assert_eq!(manager.state_number(), 0);
    }

    #[test]
    fn test_register_validator() {
        let mut manager = ValidatorManager::new();
        let address = "XNTTestAddress1".to_string();
        let public_key = vec![1u8; 32];
        let machine_id = "machine1".to_string();

        let result = manager.register_validator(address.clone(), public_key, machine_id);
        assert!(result.is_ok());
        assert_eq!(manager.get_pending_validators().len(), 1);
        assert_eq!(manager.get_active_validators().len(), 0);
    }

    #[test]
    fn test_enable_validator() {
        let mut manager = ValidatorManager::new();
        let address = "XNTTestAddress2".to_string();
        let public_key = vec![1u8; 32];
        let machine_id = "machine2".to_string();

        manager.register_validator(address.clone(), public_key, machine_id).unwrap();
        manager.enable_validator(address.clone()).unwrap();

        assert_eq!(manager.get_active_validators().len(), 1);
        assert_eq!(manager.get_pending_validators().len(), 0);
        
        let validators = manager.get_active_validators();
        assert_eq!(validators[0].state, ValidatorState::Active);
        assert!(validators[0].is_active);
    }

    #[test]
    fn test_disable_validator() {
        let mut manager = ValidatorManager::new();
        let address = "XNTTestAddress3".to_string();
        let public_key = vec![1u8; 32];
        let machine_id = "machine3".to_string();

        manager.register_validator(address.clone(), public_key, machine_id).unwrap();
        manager.enable_validator(address.clone()).unwrap();
        manager.disable_validator(address.clone()).unwrap();

        let validators = manager.get_active_validators();
        matches!(validators[0].state, ValidatorState::Suspended { .. });
        assert!(!validators[0].is_active);
    }

    #[test]
    fn test_update_uptime() {
        let mut manager = ValidatorManager::new();
        let address = "XNTTestAddress4".to_string();
        let public_key = vec![1u8; 32];
        let machine_id = "machine4".to_string();

        manager.register_validator(address.clone(), public_key, machine_id).unwrap();
        manager.enable_validator(address.clone()).unwrap();
        manager.update_uptime(address.clone(), 85).unwrap();

        let validators = manager.get_active_validators();
        assert_eq!(validators[0].uptime_score, 85);
    }

    #[test]
    fn test_update_balance() {
        let mut manager = ValidatorManager::new();
        let address = "XNTTestAddress5".to_string();
        let public_key = vec![1u8; 32];
        let machine_id = "machine5".to_string();

        manager.register_validator(address.clone(), public_key, machine_id).unwrap();
        manager.enable_validator(address.clone()).unwrap();
        manager.update_uptime(address.clone(), 90).unwrap();
        manager.update_balance(address.clone(), 1_000_000_000).unwrap(); // 10 XNT

        let validators = manager.get_active_validators();
        assert_eq!(validators[0].balance, 1_000_000_000);
        assert!(validators[0].reputation_score > 0);
        assert!(validators[0].validator_weight > 0.0);
    }

    #[test]
    fn test_committee_selection_bootstrap() {
        let mut manager = ValidatorManager::new();
        
        // Add 15 validators (below 21 threshold)
        for i in 0..15 {
            let address = format!("XNTValidator{}", i);
            let public_key = vec![i as u8; 32];
            let machine_id = format!("machine{}", i);
            
            manager.register_validator(address.clone(), public_key, machine_id).unwrap();
            manager.enable_validator(address.clone()).unwrap();
            manager.update_uptime(address.clone(), 90).unwrap();
            manager.update_balance(address.clone(), 1_000_000_000).unwrap();
        }

        let committee = manager.select_committee().unwrap();
        assert_eq!(committee.members.len(), 15); // All 15 in committee
        assert_eq!(committee.threshold, 0.67);
    }

    #[test]
    fn test_committee_selection_full() {
        let mut manager = ValidatorManager::new();
        
        // Add 30 validators (above 21 threshold)
        for i in 0..30 {
            let address = format!("XNTValidator{}", i);
            let public_key = vec![i as u8; 32];
            let machine_id = format!("machine{}", i);
            
            manager.register_validator(address.clone(), public_key, machine_id).unwrap();
            manager.enable_validator(address.clone()).unwrap();
            manager.update_uptime(address.clone(), 90).unwrap();
            manager.update_balance(address.clone(), 1_000_000_000).unwrap();
        }

        let committee = manager.select_committee().unwrap();
        assert_eq!(committee.members.len(), 30); // Capped at 30 since active < 51
        assert_eq!(committee.threshold, 0.67);
    }

    #[test]
    fn test_committee_size_capping() {
        let mut manager = ValidatorManager::new();
        
        // Add 60 validators (above 51 cap)
        for i in 0..60 {
            let address = format!("XNTValidator{}", i);
            let public_key = vec![i as u8; 32];
            let machine_id = format!("machine{}", i);
            
            manager.register_validator(address.clone(), public_key, machine_id).unwrap();
            manager.enable_validator(address.clone()).unwrap();
            manager.update_uptime(address.clone(), 90).unwrap();
            manager.update_balance(address.clone(), 1_000_000_000).unwrap();
        }

        let committee = manager.select_committee().unwrap();
        assert_eq!(committee.members.len(), 51); // Capped at 51
        assert_eq!(committee.threshold, 0.67);
    }

    #[test]
    fn test_vote_counter_creation() {
        let counter = VoteCounter::new(1);
        assert_eq!(counter.proposal_id(), 1);
        assert_eq!(counter.approve_count(), 0);
        assert_eq!(counter.reject_count(), 0);
        assert_eq!(counter.get_result(), GovernanceStatus::Proposed);
    }

    #[test]
    fn test_vote_aggregation_approval() {
        let mut counter = VoteCounter::new(1);
        
        counter.add_vote("addr1".to_string(), true, 100.0);
        counter.add_vote("addr2".to_string(), true, 100.0);
        counter.add_vote("addr3".to_string(), true, 100.0);

        assert_eq!(counter.approve_count(), 300);
        assert_eq!(counter.reject_count(), 0);
        assert_eq!(counter.get_result(), GovernanceStatus::Approved);
    }

    #[test]
    fn test_vote_aggregation_rejection() {
        let mut counter = VoteCounter::new(1);
        
        counter.add_vote("addr1".to_string(), false, 100.0);
        counter.add_vote("addr2".to_string(), false, 100.0);
        counter.add_vote("addr3".to_string(), true, 10.0);

        assert_eq!(counter.approve_count(), 10);
        assert_eq!(counter.reject_count(), 200);
        assert_eq!(counter.get_result(), GovernanceStatus::Rejected);
    }

    #[test]
    fn test_vote_aggregation_continuing() {
        let mut counter = VoteCounter::new(1);
        
        counter.add_vote("addr1".to_string(), true, 100.0);
        counter.add_vote("addr2".to_string(), false, 100.0);

        assert_eq!(counter.approve_count(), 100);
        assert_eq!(counter.reject_count(), 100);
        assert_eq!(counter.get_result(), GovernanceStatus::Voting);
    }

    #[test]
    fn test_committee_rotation() {
        let mut manager = ValidatorManager::new();
        
        // Add validators
        for i in 0..10 {
            let address = format!("XNTValidator{}", i);
            let public_key = vec![i as u8; 32];
            let machine_id = format!("machine{}", i);
            
            manager.register_validator(address.clone(), public_key, machine_id).unwrap();
            manager.enable_validator(address.clone()).unwrap();
            manager.update_uptime(address.clone(), 90).unwrap();
            manager.update_balance(address.clone(), 1_000_000_000).unwrap();
        }

        // Select committee
        let _committee1 = manager.select_committee().unwrap();
        assert!(!manager.should_rotate_committee());

        // Simulate time passing (set selection time in past)
        if let Some(ref mut committee) = manager.current_committee {
            committee.selection_time = current_timestamp() - 90000; // 25 hours ago
        }

        assert!(manager.should_rotate_committee());
    }

    #[test]
    fn test_is_committee_member() {
        let mut manager = ValidatorManager::new();
        
        let address = "XNTTestAddress6".to_string();
        let public_key = vec![1u8; 32];
        let machine_id = "machine6".to_string();

        manager.register_validator(address.clone(), public_key, machine_id).unwrap();
        manager.enable_validator(address.clone()).unwrap();
        manager.update_uptime(address.clone(), 90).unwrap();
        manager.update_balance(address.clone(), 1_000_000_000).unwrap();

        manager.select_committee().unwrap();
        assert!(manager.is_committee_member(&address));
    }

    #[test]
    fn test_governance_voting_session() {
        let mut manager = ValidatorManager::new();
        
        let proposal = GovernanceProposal {
            proposal_id: 1,
            proposer: "XNTProposer".to_string(),
            title: "Test Proposal".to_string(),
            description: "Test".to_string(),
            proposed_changes: serde_json::Value::Object(serde_json::Map::new()),
            created_at: chrono::Utc::now(),
            voting_start: chrono::Utc::now(),
            voting_end: chrono::Utc::now(),
            activation_delay_days: 30,
            approve_votes: 0,
            reject_votes: 0,
            status: GovernanceStatus::Proposed,
        };

        manager.start_voting(proposal);
        manager.add_vote(true, "addr1".to_string(), 100.0).unwrap();
        manager.add_vote(true, "addr2".to_string(), 100.0).unwrap();

        assert_eq!(manager.get_vote_result(), GovernanceStatus::Approved);
    }
}
