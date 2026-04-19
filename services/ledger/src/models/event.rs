use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Event {
    pub id: Uuid,
    pub run_id: Uuid,
    pub seq: i64,
    pub kind: String,
    pub body: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
    pub actor_id: String,
    #[serde(with = "crate::hash::hex_bytes_opt")]
    pub prev_hash: Option<Vec<u8>>,
    #[serde(with = "crate::hash::hex_bytes")]
    pub row_hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEvent {
    pub kind: String,
    #[serde(default)]
    pub body: serde_json::Value,
    #[serde(default = "Utc::now")]
    pub occurred_at: DateTime<Utc>,
}
