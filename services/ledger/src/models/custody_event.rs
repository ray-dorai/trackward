use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CustodyEvent {
    pub id: Uuid,
    pub evidence_type: String,
    pub evidence_id: Uuid,
    pub action: String,
    pub actor: String,
    pub reason: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub actor_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCustodyEvent {
    pub evidence_type: String,
    pub evidence_id: Uuid,
    pub action: String,
    pub actor: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default = "Utc::now")]
    pub occurred_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
