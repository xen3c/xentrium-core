use sha2::{Digest, Sha256};
use xentrium_shared::{NetworkState, ConsensusPhase};
use chrono::Utc;
use hex;

pub struct StateEngine {
    current_state: NetworkState,
}

impl StateEngine {
    pub fn new() -> Self {
        let current_state = NetworkState {
            state_number: 0,
            state_hash: String::new(),
            snapshot_hash: String::new(),
            transaction_window_hash: String::new(),
            accounts: std::collections::HashMap::new(),
            active_validators: vec![],
            pending_validators: vec![],
            reward_pool_balance: xentrium_shared::REWARD_POOL as i64,
            last_reward_distribution: 0,
            active_proposals: vec![],
            proposal_votes: std::collections::HashMap::new(),
            timestamp: Utc::now().timestamp(),
            committee_signature: None,
        };

        StateEngine { current_state }
    }

    pub fn current_state(&self) -> &NetworkState {
        &self.current_state
    }

    pub fn state_hash(&self) -> String {
        let hash_input = format!(
            "{}{}{}{}{}{}",
            self.current_state.state_number,
            self.current_state.accounts.len(),
            self.current_state.active_validators.len(),
            self.current_state.reward_pool_balance,
            self.current_state.timestamp,
            self.current_state.snapshot_hash
        );
        let mut hasher = Sha256::new();
        hasher.update(hash_input.as_bytes());
        hex::encode(hasher.finalize())
    }

    pub fn consensus_phase(&self) -> ConsensusPhase {
        let active_count = self.current_state.active_validators.len();
        if active_count < xentrium_shared::CONSENSUS_ACTIVATION_THRESHOLD {
            ConsensusPhase::Bootstrap
        } else {
            ConsensusPhase::FullConsensus
        }
    }

    pub fn committee_size(&self) -> usize {
        let active_count = self.current_state.active_validators.len();
        std::cmp::min(xentrium_shared::MAX_COMMITTEE_SIZE, active_count)
    }

    pub fn candidate_pool_size(&self) -> usize {
        let active_count = self.current_state.active_validators.len();
        std::cmp::min(xentrium_shared::MAX_CANDIDATE_POOL, active_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_hash() {
        let engine = StateEngine::new();
        let hash = engine.state_hash();
        assert!(!hash.is_empty());
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_consensus_phase_bootstrap() {
        let engine = StateEngine::new();
        assert_eq!(engine.consensus_phase(), ConsensusPhase::Bootstrap);
    }

    #[test]
    fn test_committee_size() {
        let engine = StateEngine::new();
        assert_eq!(engine.committee_size(), 0); // No validators
    }
}
