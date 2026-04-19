use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PromptVersion {
    pub id: Uuid,
    pub workflow: String,
    pub version: String,
    pub git_sha: String,
    pub content_hash: String,
    pub metadata: serde_json::Value,
    pub actor_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePromptVersion {
    pub workflow: String,
    pub version: String,
    pub git_sha: String,
    pub content_hash: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
