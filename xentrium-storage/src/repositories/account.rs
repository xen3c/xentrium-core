use sqlx::{SqlitePool, Row};
use xentrium_shared::{Account, Address, Balance, Result, XentriumError};

pub struct AccountRepository {
    pool: SqlitePool,
}

impl AccountRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create(&self, account: &Account) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO accounts (address, public_key, balance, nonce, is_validator, 
                                 validator_state, validator_enabled_at, uptime_score,
                                 reputation_score, validator_weight, created_at, last_activity)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&account.address)
        .bind(&account.public_key)
        .bind(account.balance)
        .bind(account.nonce as i64)
        .bind(account.is_validator as i32)
        .bind(format!("{:?}", account.validator_state))
        .bind(account.validator_enabled_at)
        .bind(account.uptime_score)
        .bind(account.reputation_score)
        .bind(account.validator_weight)
        .bind(account.created_at)
        .bind(account.last_activity)
        .execute(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, address: &Address) -> Result<Option<Account>> {
        let row = sqlx::query(
            "SELECT * FROM accounts WHERE address = ?"
        )
        .bind(address)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        if let Some(row) = row {
            Ok(Some(Account {
                address: row.try_get("address").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                public_key: row.try_get("public_key").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                balance: row.try_get("balance").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                nonce: row.try_get::<i64, _>("nonce").map_err(|e| XentriumError::DatabaseError(e.to_string()))? as u64,
                is_validator: row.try_get::<i32, _>("is_validator").map_err(|e| XentriumError::DatabaseError(e.to_string()))? != 0,
                validator_state: parse_validator_state(&row.try_get::<String, _>("validator_state").map_err(|e| XentriumError::DatabaseError(e.to_string()))?),
                validator_enabled_at: row.try_get("validator_enabled_at").ok(),
                uptime_score: row.try_get("uptime_score").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                reputation_score: row.try_get("reputation_score").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                validator_weight: row.try_get("validator_weight").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                created_at: row.try_get("created_at").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                last_activity: row.try_get("last_activity").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn update_balance(&self, address: &Address, new_balance: Balance) -> Result<()> {
        sqlx::query("UPDATE accounts SET balance = ? WHERE address = ?")
            .bind(new_balance)
            .bind(address)
            .execute(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn increment_nonce(&self, address: &Address) -> Result<u64> {
        sqlx::query("UPDATE accounts SET nonce = nonce + 1 WHERE address = ?")
            .bind(address)
            .execute(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        let account = self.get(address).await?.ok_or_else(|| XentriumError::DatabaseError("Account not found".to_string()))?;
        Ok(account.nonce)
    }
}

fn parse_validator_state(s: &str) -> xentrium_shared::ValidatorState {
    match s {
        "Active" => xentrium_shared::ValidatorState::Active,
        "Suspended" => xentrium_shared::ValidatorState::Suspended { until: 0 },
        _ => xentrium_shared::ValidatorState::Pending,
    }
}
