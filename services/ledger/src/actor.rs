//! Caller identity stamped onto every append-only row.
//!
//! Every write handler takes an `Actor` extractor. `Actor` is populated from
//! the `X-Trackward-Actor` request header at write time; the bound value is
//! persisted as `actor_id` on the row. This is the "who did this" axis of
//! the ledger.
//!
//! Absent-header policy is controlled by `AppState.default_actor`:
//!
//! * `Some(default)` — the header is optional; missing header falls through
//!   to the configured default. This is how the test harness and local
//!   bring-up run: they set `LEDGER_DEFAULT_ACTOR=test` (or similar) so
//!   pre-Phase-8c callers keep working without being taught about the
//!   header in lockstep.
//! * `None` — the header is mandatory; a missing header returns `400 Bad
//!   Request` with a clear message, rather than silently writing `'legacy'`
//!   or whatever the default column default happened to be. Production
//!   ledgers run in this strict mode.
//!
//! The header value is trimmed, rejected if empty, and capped at 256 bytes
//! so a rogue caller can't stuff a megabyte of arbitrary bytes into every
//! row. Anything beyond that is rejected with 400.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::AppState;

/// Request header carrying the caller's identity. Case-insensitive per HTTP,
/// but canonicalize the spelling in code so grep/logs stay consistent.
pub const ACTOR_HEADER: &str = "x-trackward-actor";

/// Upper bound on actor_id length. 256 bytes is plenty for
/// `service/subject@component` style identifiers; anything larger is almost
/// certainly malformed or malicious.
pub const MAX_ACTOR_LEN: usize = 256;

/// Caller identity attached to a single write. The inner string is the value
/// that lands in `actor_id` on the persisted row.
#[derive(Debug, Clone)]
pub struct Actor(pub String);

impl FromRequestParts<AppState> for Actor {
    type Rejection = ActorRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if let Some(raw) = parts.headers.get(ACTOR_HEADER) {
            let s = raw.to_str().map_err(|_| ActorRejection::Invalid)?;
            let trimmed = s.trim();
            if trimmed.is_empty() || trimmed.len() > MAX_ACTOR_LEN {
                return Err(ActorRejection::Invalid);
            }
            return Ok(Actor(trimmed.to_string()));
        }

        if let Some(default) = state.default_actor.as_deref() {
            return Ok(Actor(default.to_string()));
        }

        Err(ActorRejection::Missing)
    }
}

#[derive(Debug)]
pub enum ActorRejection {
    Missing,
    Invalid,
}

impl IntoResponse for ActorRejection {
    fn into_response(self) -> Response {
        let msg = match self {
            Self::Missing => "missing X-Trackward-Actor header",
            Self::Invalid => "invalid X-Trackward-Actor header",
        };
        let body = serde_json::json!({ "error": msg });
        (StatusCode::BAD_REQUEST, axum::Json(body)).into_response()
    }
}
