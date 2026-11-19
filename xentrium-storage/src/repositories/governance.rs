use sqlx::{SqlitePool, Row};
use xentrium_shared::{GovernanceProposal, GovernanceStatus, Address, Result, XentriumError};

pub struct GovernanceRepository {
    pool: SqlitePool,
}

impl GovernanceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, proposal: &GovernanceProposal) -> Result<()> {
        let proposed_changes = serde_json::to_string(&proposal.proposed_changes)
            .map_err(|e| XentriumError::SerializationError(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO governance_proposals (proposal_id, proposer, title, description, proposed_changes,
                                             created_at, voting_start, voting_end, activation_delay_days,
                                             approve_votes, reject_votes, status)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(proposal.proposal_id as i64)
        .bind(&proposal.proposer)
        .bind(&proposal.title)
        .bind(&proposal.description)
        .bind(&proposed_changes)
        .bind(proposal.created_at.to_rfc3339())
        .bind(proposal.voting_start.to_rfc3339())
        .bind(proposal.voting_end.to_rfc3339())
        .bind(proposal.activation_delay_days as i64)
        .bind(proposal.approve_votes as i64)
        .bind(proposal.reject_votes as i64)
        .bind(format!("{:?}", proposal.status))
        .execute(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, proposal_id: u64) -> Result<Option<GovernanceProposal>> {
        let row = sqlx::query("SELECT * FROM governance_proposals WHERE proposal_id = ?")
            .bind(proposal_id as i64)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        if let Some(row) = row {
            let pid: i64 = row.try_get("proposal_id").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let proposer: String = row.try_get("proposer").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let title: String = row.try_get("title").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let description: String = row.try_get("description").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let proposed_changes: String = row.try_get("proposed_changes").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let created_at: String = row.try_get("created_at").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let voting_start: String = row.try_get("voting_start").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let voting_end: String = row.try_get("voting_end").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let activation_delay_days: i32 = row.try_get("activation_delay_days").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let approve_votes: i64 = row.try_get("approve_votes").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let reject_votes: i64 = row.try_get("reject_votes").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let status: String = row.try_get("status").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

            let status = match status.as_str() {
                "Proposed" => GovernanceStatus::Proposed,
                "Voting" => GovernanceStatus::Voting,
                "Approved" => GovernanceStatus::Approved,
                "Rejected" => GovernanceStatus::Rejected,
                "Activated" => GovernanceStatus::Activated,
                _ => return Err(XentriumError::GovernanceError("Unknown status".to_string())),
            };

            let proposed_changes = serde_json::from_str(&proposed_changes)
                .map_err(|e| XentriumError::SerializationError(e.to_string()))?;

            Ok(Some(GovernanceProposal {
                proposal_id: pid as u64,
                proposer,
                title,
                description,
                proposed_changes,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                    .map_err(|e| XentriumError::SerializationError(e.to_string()))?
                    .with_timezone(&chrono::Utc),
                voting_start: chrono::DateTime::parse_from_rfc3339(&voting_start)
                    .map_err(|e| XentriumError::SerializationError(e.to_string()))?
                    .with_timezone(&chrono::Utc),
                voting_end: chrono::DateTime::parse_from_rfc3339(&voting_end)
                    .map_err(|e| XentriumError::SerializationError(e.to_string()))?
                    .with_timezone(&chrono::Utc),
                activation_delay_days: activation_delay_days as u32,
                approve_votes: approve_votes as u64,
                reject_votes: reject_votes as u64,
                status,
            }))
        } else {
            Ok(None)
        }
    }
}
