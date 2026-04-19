use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct RunVersionBinding {
    pub run_id: Uuid,
    pub prompt_version_id: Option<Uuid>,
    pub policy_version_id: Option<Uuid>,
    pub eval_result_id: Option<Uuid>,
    pub bound_at: DateTime<Utc>,
    pub actor_id: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRunVersionBinding {
    #[serde(default)]
    pub prompt_version_id: Option<Uuid>,
    #[serde(default)]
    pub policy_version_id: Option<Uuid>,
    #[serde(default)]
    pub eval_result_id: Option<Uuid>,
}
