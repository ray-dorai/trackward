use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Artifact {
    pub id: Uuid,
    pub run_id: Uuid,
    pub sha256: String,
    pub size_bytes: i64,
    pub media_type: String,
    pub label: String,
    pub metadata: serde_json::Value,
    pub actor_id: String,
    #[serde(with = "crate::hash::hex_bytes_opt")]
    pub prev_hash: Option<Vec<u8>>,
    #[serde(with = "crate::hash::hex_bytes")]
    pub row_hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
}
