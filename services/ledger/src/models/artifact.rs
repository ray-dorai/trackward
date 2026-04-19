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
    pub created_at: DateTime<Utc>,
}
