use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct BiasSlice {
    pub id: Uuid,
    pub run_id: Option<Uuid>,
    pub eval_result_id: Option<Uuid>,
    pub label: String,
    pub value: Option<String>,
    pub score: Option<f64>,
    pub metadata: serde_json::Value,
    pub actor_id: String,
    #[serde(with = "crate::hash::hex_bytes_opt")]
    pub prev_hash: Option<Vec<u8>>,
    #[serde(with = "crate::hash::hex_bytes")]
    pub row_hash: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBiasSlice {
    #[serde(default)]
    pub run_id: Option<Uuid>,
    #[serde(default)]
    pub eval_result_id: Option<Uuid>,
    pub label: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}
