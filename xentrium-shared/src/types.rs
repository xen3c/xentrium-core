use chrono::{DateTime, Utc};
use ed25519_dalek::{VerifyingKey, Signature, Verifier};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;

/// Address type
pub type Address = String;

/// Balance type (internal: 8 decimals)
pub type Balance = i64;

/// Nonce type
pub type Nonce = u64;

/// Transaction types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionType {
    Transfer,
    EnableValidator,
    DisableValidator,
    GovernanceVote,
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionType::Transfer => write!(f, "Transfer"),
            TransactionType::EnableValidator => write!(f, "EnableValidator"),
            TransactionType::DisableValidator => write!(f, "DisableValidator"),
            TransactionType::GovernanceVote => write!(f, "GovernanceVote"),
        }
    }
}

/// Transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub tx_type: TransactionType,
    pub from_address: Address,
    pub to_address: Option<Address>,
    pub amount: Balance,
    pub fee: Balance,
    pub nonce: Nonce,
    pub timestamp: DateTime<Utc>,
    pub signature: Option<Vec<u8>>, // Ed25519 signature as bytes for Dalek 2.x
    pub validator_data: Option<ValidatorRegistrationData>,
    pub governance_data: Option<GovernanceVoteData>,
}

impl Transaction {
    pub const DEFAULT_FEE: Balance = 10_000;      // 0.0001 XNT * 10^8
    pub const FEE_FLOOR: Balance = 1;             // 0.00000001 XNT

    pub fn calculate_total(&self) -> Balance {
        self.amount + self.fee
    }

    pub fn display_amount(&self) -> f64 {
        self.amount as f64 / 100_000_000.0
    }

    pub fn hash(&self) -> Vec<u8> {
        let hash_input = format!(
            "{}{}{}{}{}{}{}{}",
            self.tx_type,
            self.from_address,
            self.to_address.as_ref().unwrap_or(&String::new()),
            self.amount,
            self.fee,
            self.nonce,
            self.timestamp.timestamp(),
            self.validator_data.as_ref().map(|d| &d.machine_id).unwrap_or(&String::new())
        );
        let mut hasher = Sha256::new();
        hasher.update(hash_input.as_bytes());
        hasher.finalize().to_vec()
    }

    pub fn verify_signature(&self, public_key: &VerifyingKey) -> bool {
        if let Some(sig_bytes) = &self.signature {
            let hash = self.hash();
            if sig_bytes.len() == 64 {
                let mut sig_array = [0u8; 64];
                sig_array.copy_from_slice(sig_bytes);
                let signature = Signature::from_bytes(&sig_array);
                public_key.verify(&hash, &signature).is_ok()
            } else {
                false
            }
        } else {
            false
        }
    }
}

/// Validator registration data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorRegistrationData {
    pub machine_id: String,
    pub endpoint: String,
}

/// Governance vote data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceVoteData {
    pub proposal_id: String,
    pub vote: bool,
}

/// Validator state
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ValidatorState {
    Pending,
    Active,
    Suspended { until: i64 },
}

/// Account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub address: Address,
    pub public_key: Vec<u8>,
    pub balance: Balance,
    pub nonce: Nonce,
    pub is_validator: bool,
    pub validator_state: ValidatorState,
    pub validator_enabled_at: Option<i64>,
    pub uptime_score: i32,
    pub reputation_score: i64,
    pub validator_weight: f64,
    pub created_at: i64,
    pub last_activity: i64,
}

impl Account {
    pub fn display_balance(&self) -> f64 {
        self.balance as f64 / 100_000_000.0
    }

    pub fn balance_in_xnt(&self) -> i64 {
        self.balance / 100_000_000
    }

    pub fn calculate_balance_score(&self) -> i64 {
        self.balance_in_xnt()
    }

    pub fn calculate_reputation_score(&self) -> i64 {
        let balance_score = self.calculate_balance_score();
        let uptime_bonus = (self.uptime_score as i64) * 100;
        balance_score + uptime_bonus
    }

    pub fn calculate_validator_weight(&self) -> f64 {
        let reputation = self.calculate_reputation_score() as f64;
        if reputation <= 0.0 {
            0.0
        } else {
            reputation.sqrt()
        }
    }
}

/// Network state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkState {
    pub state_number: i64,
    pub state_hash: String,
    pub snapshot_hash: String,
    pub transaction_window_hash: String,
    pub accounts: std::collections::HashMap<Address, Account>,
    pub active_validators: Vec<Address>,
    pub pending_validators: Vec<Address>,
    pub reward_pool_balance: Balance,
    pub last_reward_distribution: i64,
    pub active_proposals: Vec<String>,
    pub proposal_votes: std::collections::HashMap<String, ProposalVote>,
    pub timestamp: i64,
    pub committee_signature: Option<String>,
}

/// Proposal vote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalVote {
    pub proposal_id: String,
    pub voter_address: Address,
    pub vote: bool,
    pub timestamp: i64,
}

/// Validator info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub address: Address,
    pub public_key: Vec<u8>,
    pub machine_id: String,
    pub balance: Balance,
    pub state: ValidatorState,
    pub enabled_at: Option<i64>,
    pub uptime_score: i32,
    pub reputation_score: i64,
    pub validator_weight: f64,
    pub last_seen: i64,
    pub consecutive_uptime_weeks: i32,
    pub committees_participated: i32,
    pub votes_cast: i32,
    pub votes_correct: i32,
    pub endpoint: String,
    pub is_active: bool,
}

/// Governance proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceProposal {
    pub proposal_id: u64,
    pub proposer: Address,
    pub title: String,
    pub description: String,
    pub proposed_changes: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub voting_start: DateTime<Utc>,
    pub voting_end: DateTime<Utc>,
    pub activation_delay_days: u32,
    pub approve_votes: u64,
    pub reject_votes: u64,
    pub status: GovernanceStatus,
}

/// Governance status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GovernanceStatus {
    Proposed,
    Voting,
    Approved,
    Rejected,
    Activated,
}

/// Snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub state_number: i64,
    pub state_hash: String,
    pub snapshot_hash: String,
    pub timestamp: i64,
    pub accounts: std::collections::HashMap<Address, Account>,
    pub active_validators: Vec<Address>,
    pub pending_validators: Vec<Address>,
    pub reward_pool_balance: Balance,
    pub last_reward_distribution: i64,
    pub active_proposals: Vec<String>,
    pub proposal_votes: std::collections::HashMap<String, ProposalVote>,
    pub committee_signature: Option<Vec<u8>>,
}

/// Consensus phase
#[derive(Debug, Clone, PartialEq)]
pub enum ConsensusPhase {
    Bootstrap,
    FullConsensus,
}
