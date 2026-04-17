use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PolicyVersion {
    pub id: Uuid,
    pub scope: String,
    pub version: String,
    pub git_sha: String,
    pub content_hash: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePolicyVersion {
    pub scope: String,
    pub version: String,
    pub git_sha: String,
    pub content_hash: String,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
