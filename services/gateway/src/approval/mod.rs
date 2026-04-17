pub mod gates;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde_json::json;
use tokio::sync::{oneshot, Mutex};
use uuid::{NoContext, Timestamp, Uuid};

use crate::errors::Error;
use crate::tool_proxy::provenance::{resolve_or_mint_run, RUN_ID_HEADER};
use crate::AppState;

/// An in-memory store of pending approvals. Phase 4 will persist this.
#[derive(Clone, Default)]
pub struct ApprovalStore {
    pub pending: Arc<Mutex<HashMap<Uuid, PendingApproval>>>,
}

/// A pending approval, waiting for a human to decide. The `decision_tx`
/// is a oneshot — first decide wins, any later decide on the same id is a 404.
pub struct PendingApproval {
    pub run_id: Uuid,
    pub tool: String,
    pub decision_tx: oneshot::Sender<Decision>,
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub granted: bool,
    pub reason: Option<String>,
}

/// Mint a new time-ordered approval id.
fn new_approval_id() -> Uuid {
    Uuid::new_v7(Timestamp::now(NoContext))
}

/// `POST /approval/request` — blocks until `decide` is called for this approval.
pub async fn request(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Response, Error> {
    let (run_id, _minted) = resolve_or_mint_run(&state, &headers, "approval").await?;

    let approval_id = new_approval_id();
    let tool = body
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let (tx, rx) = oneshot::channel::<Decision>();

    // Stash the pending approval *before* recording the event, so a very
    // fast decider never races ahead of our own bookkeeping.
    {
        let mut pending = state.approvals.pending.lock().await;
        pending.insert(
            approval_id,
            PendingApproval {
                run_id,
                tool: tool.clone(),
                decision_tx: tx,
            },
        );
    }

    // Record the request. Mirror the caller's payload plus the id we minted.
    let mut requested_body = body.clone();
    if let Some(obj) = requested_body.as_object_mut() {
        obj.insert("approval_id".into(), json!(approval_id.to_string()));
    } else {
        requested_body = json!({
            "approval_id": approval_id.to_string(),
            "payload": body,
        });
    }
    state
        .ledger
        .append_event(run_id, "approval_requested", requested_body)
        .await?;

    // Block until a decision is posted. If the sender is dropped without
    // sending (e.g. the store is dropped mid-flight), surface that as an error
    // rather than hanging forever.
    let decision = rx
        .await
        .map_err(|_| Error::Internal("approval channel closed before decision".into()))?;

    // Record the outcome.
    let kind = if decision.granted {
        "approval_granted"
    } else {
        "approval_denied"
    };
    state
        .ledger
        .append_event(
            run_id,
            kind,
            json!({
                "approval_id": approval_id.to_string(),
                "tool": tool,
                "reason": decision.reason,
            }),
        )
        .await?;

    let decision_str = if decision.granted { "granted" } else { "denied" };
    let body = json!({
        "approval_id": approval_id.to_string(),
        "decision": decision_str,
        "reason": decision.reason,
    });
    Ok(Response::builder()
        .status(axum::http::StatusCode::OK)
        .header(RUN_ID_HEADER, run_id.to_string())
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(axum::body::Body::from(body.to_string()))
        .map_err(|e| Error::Internal(e.to_string()))?)
}

/// `POST /approval/{id}/decide` — unblock the pending requester.
pub async fn decide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, Error> {
    let pending = {
        let mut map = state.approvals.pending.lock().await;
        map.remove(&id).ok_or(Error::NotFound)?
    };

    let decision_str = body
        .get("decision")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::BadRequest("missing 'decision'".into()))?;
    let granted = match decision_str {
        "granted" => true,
        "denied" => false,
        other => {
            return Err(Error::BadRequest(format!(
                "decision must be 'granted' or 'denied', got {other:?}"
            )));
        }
    };
    let reason = body
        .get("reason")
        .and_then(|v| v.as_str())
        .map(String::from);

    // If the waiter is gone (request timed out / was cancelled), the send
    // fails — we've already removed it from the map, so just report 404-ish.
    pending
        .decision_tx
        .send(Decision { granted, reason })
        .map_err(|_| Error::NotFound)?;

    Ok(Json(json!({
        "ok": true,
        "run_id": pending.run_id.to_string(),
        "tool": pending.tool,
    })))
}
