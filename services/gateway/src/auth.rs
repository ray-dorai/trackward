//! Bearer-token gate for the gateway's own endpoints. Symmetric with the
//! ledger middleware — when `AppState.config.auth_token` is Some, every
//! request routed through this middleware must carry
//! `Authorization: Bearer <token>`. When None, everything passes (dev
//! default).
//!
//! `/health` is wired outside this middleware so load balancers can probe
//! without a secret.

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
    let Some(expected) = state.config.auth_token.as_deref() else {
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
