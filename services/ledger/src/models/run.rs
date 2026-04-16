use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Run {
    pub id: Uuid,
    pub agent: String,
    pub started_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateRun {
    pub agent: String,
    #[serde(default = "Utc::now")]
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
