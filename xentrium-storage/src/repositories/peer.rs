use sqlx::SqlitePool;
use xentrium_shared::{Result, XentriumError};

pub struct PeerRepository {
    pool: SqlitePool,
}

#[derive(Clone)]
pub struct StoredPeer {
    pub address: String,
    pub endpoint: String,
    pub machine_id: String,
    pub state: String,
    pub last_seen: i64,
    pub uptime_score: i64,
    pub is_bootstrap: bool,
}

impl PeerRepository {
    pub fn new(pool: SqlitePool) -> Self {
        PeerRepository { pool }
    }

    pub async fn store(&self, peer: &StoredPeer) -> Result<()> {
        sqlx::query(
            "INSERT OR REPLACE INTO peers (address, endpoint, machine_id, state, last_seen, uptime_score, is_bootstrap) VALUES (?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(&peer.address)
        .bind(&peer.endpoint)
        .bind(&peer.machine_id)
        .bind(&peer.state)
        .bind(peer.last_seen)
        .bind(peer.uptime_score)
        .bind(peer.is_bootstrap)
        .execute(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn get(&self, address: &str) -> Result<Option<StoredPeer>> {
        let row = sqlx::query_as::<_, (String, String, String, String, i64, i64, bool)>(
            "SELECT address, endpoint, machine_id, state, last_seen, uptime_score, is_bootstrap FROM peers WHERE address = ?"
        )
        .bind(address)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(row.map(|(address, endpoint, machine_id, state, last_seen, uptime_score, is_bootstrap)| {
            StoredPeer {
                address,
                endpoint,
                machine_id,
                state,
                last_seen,
                uptime_score,
                is_bootstrap,
            }
        }))
    }

    pub async fn get_all(&self) -> Result<Vec<StoredPeer>> {
        let rows = sqlx::query_as::<_, (String, String, String, String, i64, i64, bool)>(
            "SELECT address, endpoint, machine_id, state, last_seen, uptime_score, is_bootstrap FROM peers"
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(rows.into_iter().map(|(address, endpoint, machine_id, state, last_seen, uptime_score, is_bootstrap)| {
            StoredPeer {
                address,
                endpoint,
                machine_id,
                state,
                last_seen,
                uptime_score,
                is_bootstrap,
            }
        }).collect())
    }

    pub async fn delete(&self, address: &str) -> Result<()> {
        sqlx::query("DELETE FROM peers WHERE address = ?")
            .bind(address)
            .execute(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn clear(&self) -> Result<()> {
        sqlx::query("DELETE FROM peers")
            .execute(&self.pool)
            .await
            .map_err(|e| XentriumError::DatabaseError(e.to_string()))?;

        Ok(())
    }
}
