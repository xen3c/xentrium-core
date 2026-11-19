use sqlx::{SqlitePool, Row};
use xentrium_shared::{Snapshot, Result, XentriumError};

pub struct SnapshotRepository {
    pool: SqlitePool,
}

impl SnapshotRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn store(&self, snapshot: &Snapshot) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO snapshots (state_number, state_hash, snapshot_hash, timestamp, committee_signature)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(snapshot.state_number)
        .bind(&snapshot.state_hash)
        .bind(&snapshot.snapshot_hash)
        .bind(snapshot.timestamp)
        .bind(&snapshot.committee_signature)
        .execute(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn get_latest(&self) -> Result<Option<Snapshot>> {
        let row = sqlx::query("SELECT * FROM snapshots ORDER BY state_number DESC LIMIT 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        if let Some(row) = row {
            Ok(Some(Snapshot {
                state_number: row.try_get("state_number").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                state_hash: row.try_get("state_hash").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                snapshot_hash: row.try_get("snapshot_hash").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                timestamp: row.try_get("timestamp").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                accounts: std::collections::HashMap::new(), // Simplified for Phase 1
                active_validators: vec![],
                pending_validators: vec![],
                reward_pool_balance: 0,
                last_reward_distribution: 0,
                active_proposals: vec![],
                proposal_votes: std::collections::HashMap::new(),
                committee_signature: row.try_get("committee_signature").ok(),
            }))
        } else {
            Ok(None)
        }
    }
}
