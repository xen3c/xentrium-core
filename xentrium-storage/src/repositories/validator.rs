use sqlx::{SqlitePool, Row};
use xentrium_shared::{ValidatorInfo, Address, Result, XentriumError};

pub struct ValidatorRepository {
    pool: SqlitePool,
}

impl ValidatorRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn register(&self, validator: &ValidatorInfo) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR REPLACE INTO validators (address, public_key, machine_id, balance, state, enabled_at,
                                    uptime_score, reputation_score, validator_weight, last_seen,
                                    consecutive_uptime_weeks, committees_participated, votes_cast,
                                    votes_correct, endpoint, is_active)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&validator.address)
        .bind(&validator.public_key)
        .bind(&validator.machine_id)
        .bind(validator.balance)
        .bind(format!("{:?}", validator.state))
        .bind(validator.enabled_at)
        .bind(validator.uptime_score)
        .bind(validator.reputation_score)
        .bind(validator.validator_weight)
        .bind(validator.last_seen)
        .bind(validator.consecutive_uptime_weeks)
        .bind(validator.committees_participated)
        .bind(validator.votes_cast)
        .bind(validator.votes_correct)
        .bind(&validator.endpoint)
        .bind(validator.is_active as i32)
        .execute(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn get_all(&self) -> Result<Vec<ValidatorInfo>> {
        let rows = sqlx::query("SELECT * FROM validators")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        let mut validators = Vec::new();
        for row in rows {
            if let Some(v) = Self::row_to_validator(&row)? {
                validators.push(v);
            }
        }
        Ok(validators)
    }

    pub async fn get(&self, address: &Address) -> Result<Option<ValidatorInfo>> {
        let row = sqlx::query("SELECT * FROM validators WHERE address = ?")
            .bind(address)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        if let Some(row) = row {
            Self::row_to_validator(&row)
        } else {
            Ok(None)
        }
    }

    fn row_to_validator(row: &sqlx::sqlite::SqliteRow) -> Result<Option<ValidatorInfo>> {
        {
            let state_str = row.try_get::<String, _>("state").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let state = match state_str.as_str() {
                "Active" => xentrium_shared::ValidatorState::Active,
                "Suspended" => xentrium_shared::ValidatorState::Suspended { until: 0 },
                _ => xentrium_shared::ValidatorState::Pending,
            };

            Ok(Some(ValidatorInfo {
                address: row.try_get("address").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                public_key: row.try_get("public_key").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                machine_id: row.try_get("machine_id").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                balance: row.try_get("balance").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                state,
                enabled_at: row.try_get("enabled_at").ok(),
                uptime_score: row.try_get("uptime_score").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                reputation_score: row.try_get("reputation_score").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                validator_weight: row.try_get("validator_weight").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                last_seen: row.try_get("last_seen").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                consecutive_uptime_weeks: row.try_get("consecutive_uptime_weeks").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                committees_participated: row.try_get("committees_participated").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                votes_cast: row.try_get("votes_cast").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                votes_correct: row.try_get("votes_correct").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                endpoint: row.try_get("endpoint").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                is_active: row.try_get::<i32, _>("is_active").map_err(|e| XentriumError::DatabaseError(e.to_string()))? != 0,
            }))
        }
    }

    pub async fn get_active_count(&self) -> Result<usize> {
        let row = sqlx::query("SELECT COUNT(*) as count FROM validators WHERE is_active = 1")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        let count: i64 = row.try_get("count").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
        Ok(count as usize)
    }
}
