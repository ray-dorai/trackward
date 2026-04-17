use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EvalResult {
    pub id: Uuid,
    pub workflow: String,
    pub version: String,
    pub prompt_version_id: Option<Uuid>,
    pub git_sha: String,
    pub content_hash: String,
    pub passed: bool,
    pub summary: serde_json::Value,
    pub ran_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateEvalResult {
    pub workflow: String,
    pub version: String,
    #[serde(default)]
    pub prompt_version_id: Option<Uuid>,
    pub git_sha: String,
    pub content_hash: String,
    pub passed: bool,
    #[serde(default)]
    pub summary: serde_json::Value,
    #[serde(default = "Utc::now")]
    pub ran_at: DateTime<Utc>,
}
