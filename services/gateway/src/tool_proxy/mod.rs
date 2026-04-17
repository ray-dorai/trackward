pub mod provenance;
pub mod routing;
pub mod side_effects;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;
use serde_json::json;
use uuid::Uuid;

use crate::errors::Error;
use crate::AppState;
use provenance::{resolve_or_mint_run, RUN_ID_HEADER};

/// Proxy a tool call through the gateway.
///
/// Flow:
/// 1. Look up the backend URL for `tool`. 404 if unknown.
/// 2. Resolve run_id: use the caller's x-trackward-run-id header, else create a new run.
/// 3. Record a `tool_call` event in the ledger (input + tool name).
/// 4. Forward the body to the backend.
/// 5. Record either `tool_result` (2xx) or `tool_error` (non-2xx) in the ledger.
/// 6. Return the backend's status + body, stamped with x-trackward-run-id.
pub async fn proxy(
    State(state): State<AppState>,
    Path(tool): Path<String>,
    headers: HeaderMap,
    Json(input): Json<serde_json::Value>,
) -> Result<Response, Error> {
    // 1. Resolve backend — unknown tools 404 before we touch the ledger.
    let backend_url = routing::resolve(&state.config.tool_routes, &tool)
        .ok_or_else(|| Error::UnknownTool(tool.clone()))?
        .clone();

    // 2. Resolve run_id: reuse or mint a new one via the ledger.
    //    When we mint, we also stamp the run with the active registry binding.
    let (run_id, _minted) = resolve_or_mint_run(&state, &headers, "tool_proxy").await?;

    // 3. Record the call before invoking the backend.
    state
        .ledger
        .append_event(
            run_id,
            "tool_call",
            json!({ "tool": tool, "input": input }),
        )
        .await?;

    // 4. Forward to backend.
    let backend_resp = state
        .http
        .post(&backend_url)
        .json(&input)
        .send()
        .await
        .map_err(|e| Error::Backend(e.to_string()))?;
    let backend_status = backend_resp.status();
    let backend_body = backend_resp
        .bytes()
        .await
        .map_err(|e| Error::Backend(e.to_string()))?;

    // Try to parse as JSON for structured logging; fall back to raw string.
    let body_value: serde_json::Value = serde_json::from_slice(&backend_body)
        .unwrap_or_else(|_| json!({ "raw": String::from_utf8_lossy(&backend_body).to_string() }));

    // 5. Record result or error.
    if backend_status.is_success() {
        state
            .ledger
            .append_event(
                run_id,
                "tool_result",
                json!({ "tool": tool, "output": body_value }),
            )
            .await?;
    } else {
        state
            .ledger
            .append_event(
                run_id,
                "tool_error",
                json!({
                    "tool": tool,
                    "status": backend_status.as_u16(),
                    "body": body_value,
                }),
            )
            .await?;
    }

    // 6. Return caller-facing response with run_id header.
    Ok(Response::builder()
        .status(backend_status)
        .header(RUN_ID_HEADER, run_id.to_string())
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(backend_body))
        .map_err(|e| Error::Internal(e.to_string()))?)
}

// Explicitly used above for clarity; silence unused-import warning.
#[allow(dead_code)]
fn _type_guard(_: Uuid, _: StatusCode) {}
