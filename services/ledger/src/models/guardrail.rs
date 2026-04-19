use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Guardrail {
    pub id: Uuid,
    pub run_id: Uuid,
    pub name: String,
    pub stage: String,
    pub target: Option<String>,
    pub outcome: String,
    pub detail: serde_json::Value,
    pub evaluated_at: DateTime<Utc>,
    pub actor_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateGuardrail {
    pub run_id: Uuid,
    pub name: String,
    pub stage: String,
    #[serde(default)]
    pub target: Option<String>,
    pub outcome: String,
    #[serde(default)]
    pub detail: serde_json::Value,
    #[serde(default = "Utc::now")]
    pub evaluated_at: DateTime<Utc>,
}
