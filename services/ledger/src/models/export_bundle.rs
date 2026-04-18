use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ExportBundle {
    pub id: Uuid,
    pub case_id: Uuid,
    pub manifest_json: String,
    pub manifest_sha256: String,
    pub signature: String,
    pub key_id: String,
    pub public_key_hex: String,
    pub signed_by: String,
    pub signed_at: DateTime<Utc>,
    pub storage_uri: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateExportBundle {
    pub signed_by: String,
    /// Test-only hook: pin `signed_at` to make manifest bytes — and
    /// therefore the signature — deterministic for a given case.
    #[serde(default)]
    pub fixed_signed_at: Option<DateTime<Utc>>,
}
