//! Bearer-token gate. When `AppState.auth_token` is Some, every request
//! routed through this middleware must carry `Authorization: Bearer
//! <token>`. When None, every request passes — that's the dev default
//! so local work and pre-Phase-7 tests don't need to know about auth.
//!
//! Health checks are intentionally wired *outside* this middleware so
//! load balancers can probe the service without a secret.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::AppState;

pub async fn require_bearer(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(expected) = state.auth_token.as_deref() else {
        return Ok(next.run(req).await);
    };
    let ok = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(|got| constant_time_eq(got.as_bytes(), expected.as_bytes()))
        .unwrap_or(false);
    if ok {
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Avoid leaking token length / early-mismatch timing. Overkill for a
/// shared secret, but the cost is two lines.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
