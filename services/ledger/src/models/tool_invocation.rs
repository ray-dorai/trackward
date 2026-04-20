use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ToolInvocation {
    pub id: Uuid,
    pub run_id: Uuid,
    pub tool: String,
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub status: String,
    pub status_code: Option<i32>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub actor_id: String,
    #[serde(with = "crate::hash::hex_bytes_opt")]
    pub prev_hash: Option<Vec<u8>>,
    #[serde(with = "crate::hash::hex_bytes")]
    pub row_hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateToolInvocation {
    pub run_id: Uuid,
    pub tool: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub output: serde_json::Value,
    pub status: String,
    #[serde(default)]
    pub status_code: Option<i32>,
    #[serde(default = "Utc::now")]
    pub started_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub finished_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
