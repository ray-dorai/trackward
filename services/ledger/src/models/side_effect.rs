use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SideEffect {
    pub id: Uuid,
    pub run_id: Uuid,
    pub tool_invocation_id: Option<Uuid>,
    pub kind: String,
    pub target: String,
    pub status: String,
    pub confirmation: serde_json::Value,
    pub observed_at: DateTime<Utc>,
    pub actor_id: String,
    #[serde(with = "crate::hash::hex_bytes_opt")]
    pub prev_hash: Option<Vec<u8>>,
    #[serde(with = "crate::hash::hex_bytes")]
    pub row_hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSideEffect {
    pub run_id: Uuid,
    #[serde(default)]
    pub tool_invocation_id: Option<Uuid>,
    pub kind: String,
    pub target: String,
    pub status: String,
    #[serde(default)]
    pub confirmation: serde_json::Value,
    #[serde(default = "Utc::now")]
    pub observed_at: DateTime<Utc>,
}
