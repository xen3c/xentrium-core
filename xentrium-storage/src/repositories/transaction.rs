use sqlx::{SqlitePool, Row};
use xentrium_shared::{Transaction, Result, XentriumError};

pub struct TransactionRepository {
    pool: SqlitePool,
}

impl TransactionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn store(&self, tx: &Transaction) -> Result<()> {
        let tx_hash = hex::encode(tx.hash());
        let tx_type = tx.tx_type.to_string();
        let signature = tx.signature.clone();
        let validator_data = tx.validator_data.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default());
        let governance_data = tx.governance_data.as_ref().map(|d| serde_json::to_string(d).unwrap_or_default());

        sqlx::query(
            r#"
            INSERT INTO transactions (tx_hash, tx_type, from_address, to_address, amount, fee, 
                                      nonce, timestamp, signature, validator_data, governance_data)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&tx_hash)
        .bind(&tx_type)
        .bind(&tx.from_address)
        .bind(&tx.to_address)
        .bind(tx.amount)
        .bind(tx.fee)
        .bind(tx.nonce as i64)
        .bind(tx.timestamp.to_rfc3339())
        .bind(signature)
        .bind(validator_data)
        .bind(governance_data)
        .execute(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, tx_hash: &str) -> Result<Option<Transaction>> {
        let row = sqlx::query("SELECT * FROM transactions WHERE tx_hash = ?")
            .bind(tx_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        if let Some(row) = row {
            let tx_type = row.try_get::<String, _>("tx_type").map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
            let tx_type = match tx_type.as_str() {
                "Transfer" => xentrium_shared::TransactionType::Transfer,
                "EnableValidator" => xentrium_shared::TransactionType::EnableValidator,
                "DisableValidator" => xentrium_shared::TransactionType::DisableValidator,
                "GovernanceVote" => xentrium_shared::TransactionType::GovernanceVote,
                _ => return Err(XentriumError::InvalidTransaction("Unknown transaction type".to_string())),
            };

            let signature = row.try_get::<Vec<u8>, _>("signature").ok();

            let validator_data = row.try_get::<String, _>("validator_data")
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());

            let governance_data = row.try_get::<String, _>("governance_data")
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());

            Ok(Some(Transaction {
                id: tx_hash.to_string(),
                tx_type,
                from_address: row.try_get("from_address").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                to_address: row.try_get("to_address").ok(),
                amount: row.try_get("amount").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                fee: row.try_get("fee").map_err(|e| XentriumError::DatabaseError(e.to_string()))?,
                nonce: row.try_get::<i64, _>("nonce").map_err(|e| XentriumError::DatabaseError(e.to_string()))? as u64,
                timestamp: chrono::DateTime::parse_from_rfc3339(&row.try_get::<String, _>("timestamp").map_err(|e| XentriumError::DatabaseError(e.to_string()))?)
                    .map_err(|e| XentriumError::SerializationError(e.to_string()))?
                    .with_timezone(&chrono::Utc),
                signature,
                validator_data,
                governance_data,
            }))
        } else {
            Ok(None)
        }
    }
}
