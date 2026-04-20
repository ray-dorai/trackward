use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

/// A persisted merkle anchor row. See `migrations/0011_merkle_anchors.sql`
/// for the invariants the DB enforces (32-byte root_hash, 64-byte
/// signature, non-empty window, positive leaf_count).
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Anchor {
    pub id: Uuid,
    /// `Some(run_id)` for a run-scoped anchor, `None` for a global
    /// anchor covering every chained row in the window.
    pub run_id: Option<Uuid>,
    pub anchored_from: DateTime<Utc>,
    pub anchored_to: DateTime<Utc>,
    pub leaf_count: i64,
    #[serde(with = "crate::hash::hex_bytes")]
    pub root_hash: Vec<u8>,
    #[serde(with = "crate::hash::hex_bytes")]
    pub signature: Vec<u8>,
    pub key_id: String,
    pub public_key_hex: String,
    /// URI where the signed manifest landed in the external WORM sink,
    /// e.g., `s3://trackward-anchors/run/<id>/<anchor_id>.json` or
    /// `memory://<id>` in tests.
    pub anchor_target: String,
    pub anchored_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}
