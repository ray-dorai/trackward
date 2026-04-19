use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct HumanApproval {
    pub id: Uuid,
    pub run_id: Uuid,
    pub tool: String,
    pub decision: String,
    pub reason: Option<String>,
    pub decided_by: Option<String>,
    pub requested_at: DateTime<Utc>,
    pub decided_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub actor_id: String,
    #[serde(with = "crate::hash::hex_bytes_opt")]
    pub prev_hash: Option<Vec<u8>>,
    #[serde(with = "crate::hash::hex_bytes")]
    pub row_hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateHumanApproval {
    /// Caller supplies the id — the gateway mints it up front at request
    /// time so the event stream and this row share the same approval_id.
    pub id: Uuid,
    pub run_id: Uuid,
    pub tool: String,
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub decided_by: Option<String>,
    pub requested_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub decided_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
