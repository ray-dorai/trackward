use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Case {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub opened_by: String,
    pub opened_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub actor_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCase {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub opened_by: String,
    #[serde(default = "Utc::now")]
    pub opened_at: DateTime<Utc>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
