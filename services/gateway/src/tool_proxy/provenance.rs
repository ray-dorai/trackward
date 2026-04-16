use axum::http::HeaderMap;
use uuid::Uuid;

pub const RUN_ID_HEADER: &str = "x-trackward-run-id";

/// Extract a run_id from request headers, or None.
pub fn extract_run_id(headers: &HeaderMap) -> Option<Uuid> {
    headers
        .get(RUN_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}
