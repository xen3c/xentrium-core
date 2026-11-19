use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};
use xentrium_shared::{Result, XentriumError};

pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(Database { pool })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn initialize_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS accounts (
                address TEXT PRIMARY KEY,
                public_key BLOB NOT NULL,
                balance INTEGER NOT NULL DEFAULT 0,
                nonce INTEGER NOT NULL DEFAULT 0,
                is_validator INTEGER NOT NULL DEFAULT 0,
                validator_state TEXT NOT NULL DEFAULT 'Pending',
                validator_enabled_at INTEGER,
                uptime_score INTEGER NOT NULL DEFAULT 0,
                reputation_score INTEGER NOT NULL DEFAULT 0,
                validator_weight REAL NOT NULL DEFAULT 0.0,
                created_at INTEGER NOT NULL,
                last_activity INTEGER NOT NULL
            )
            "#,
        )
        .execute(self.pool())
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS transactions (
                tx_hash TEXT PRIMARY KEY,
                tx_type TEXT NOT NULL,
                from_address TEXT NOT NULL,
                to_address TEXT,
                amount INTEGER NOT NULL,
                fee INTEGER NOT NULL,
                nonce INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                signature BLOB,
                validator_data TEXT,
                governance_data TEXT
            )
            "#,
        )
        .execute(self.pool())
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS validators (
                address TEXT PRIMARY KEY,
                public_key BLOB NOT NULL,
                machine_id TEXT NOT NULL,
                balance INTEGER NOT NULL,
                state TEXT NOT NULL DEFAULT 'Pending',
                enabled_at INTEGER,
                uptime_score INTEGER NOT NULL DEFAULT 0,
                reputation_score INTEGER NOT NULL DEFAULT 0,
                validator_weight REAL NOT NULL DEFAULT 0.0,
                last_seen INTEGER NOT NULL,
                consecutive_uptime_weeks INTEGER NOT NULL DEFAULT 0,
                committees_participated INTEGER NOT NULL DEFAULT 0,
                votes_cast INTEGER NOT NULL DEFAULT 0,
                votes_correct INTEGER NOT NULL DEFAULT 0,
                endpoint TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1
            )
            "#,
        )
        .execute(self.pool())
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS snapshots (
                state_number INTEGER PRIMARY KEY,
                state_hash TEXT NOT NULL,
                snapshot_hash TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                committee_signature BLOB
            )
            "#,
        )
        .execute(self.pool())
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS governance_proposals (
                proposal_id INTEGER PRIMARY KEY,
                proposer TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT NOT NULL,
                proposed_changes TEXT NOT NULL,
                created_at TEXT NOT NULL,
                voting_start TEXT NOT NULL,
                voting_end TEXT NOT NULL,
                activation_delay_days INTEGER NOT NULL,
                approve_votes INTEGER NOT NULL DEFAULT 0,
                reject_votes INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'Proposed'
            )
            "#,
        )
        .execute(self.pool())
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS committee_members (
                address TEXT PRIMARY KEY,
                state_number INTEGER NOT NULL,
                added_at INTEGER NOT NULL
            )
            "#,
        )
        .execute(self.pool())
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS peers (
                address TEXT PRIMARY KEY,
                endpoint TEXT NOT NULL,
                machine_id TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'Unknown',
                last_seen INTEGER NOT NULL,
                uptime_score INTEGER NOT NULL DEFAULT 0,
                is_bootstrap INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(self.pool())
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}
