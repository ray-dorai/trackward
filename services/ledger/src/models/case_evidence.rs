use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CaseEvidence {
    pub case_id: Uuid,
    pub evidence_type: String,
    pub evidence_id: Uuid,
    pub linked_by: String,
    pub linked_at: DateTime<Utc>,
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCaseEvidence {
    pub evidence_type: String,
    pub evidence_id: Uuid,
    pub linked_by: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default = "Utc::now")]
    pub linked_at: DateTime<Utc>,
}
