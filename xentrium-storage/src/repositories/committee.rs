use sqlx::SqlitePool;
use xentrium_shared::{Result, XentriumError};

pub struct CommitteeRepository {
    pool: SqlitePool,
}

impl CommitteeRepository {
    pub fn new(pool: SqlitePool) -> Self {
        CommitteeRepository { pool }
    }

    pub async fn store(&self, members: &[String], state_number: i64) -> Result<()> {
        // Clear existing committee
        sqlx::query("DELETE FROM committee_members")
            .execute(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        // Insert new committee members
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        for address in members {
            sqlx::query(
                "INSERT INTO committee_members (address, state_number, added_at) VALUES (?, ?, ?)"
            )
            .bind(address)
            .bind(state_number)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;
        }

        Ok(())
    }

    pub async fn get(&self) -> Result<Vec<String>> {
        let rows = sqlx::query_as::<_, (String, i64, i64)>(
            "SELECT address, state_number, added_at FROM committee_members"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(rows.into_iter().map(|(addr, _, _)| addr).collect())
    }

    pub async fn get_state_number(&self) -> Result<i64> {
        let row = sqlx::query_as::<_, (i64,)>(
            "SELECT MAX(state_number) as state_number FROM committee_members"
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(row.map(|(sn,)| sn).unwrap_or(0))
    }
}
