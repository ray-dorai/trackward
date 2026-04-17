pub mod logging;

use axum::body::Body;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use axum::Json;
use serde_json::json;

use crate::errors::Error;
use crate::tool_proxy::provenance::{resolve_or_mint_run, RUN_ID_HEADER};
use crate::AppState;

/// Proxy a retrieval query. Records the query, forwards to the retrieval
/// backend, stores each returned doc as an immutable artifact, records the
/// result event with the artifact IDs, and returns the docs to the caller.
pub async fn retrieve(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<serde_json::Value>,
) -> Result<Response, Error> {
    let backend_url = state
        .config
        .retrieval_backend
        .clone()
        .ok_or_else(|| Error::Internal("no retrieval backend configured".into()))?;

    let (run_id, _minted) = resolve_or_mint_run(&state, &headers, "retrieval_proxy").await?;

    // 1. Query event — record the raw input so callers can introspect whatever
    //    shape they sent (query string, filters, etc).
    state
        .ledger
        .append_event(run_id, "retrieval_query", input.clone())
        .await?;

    // 2. Call backend
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

    if !backend_status.is_success() {
        state
            .ledger
            .append_event(
                run_id,
                "retrieval_error",
                json!({
                    "status": backend_status.as_u16(),
                    "body": String::from_utf8_lossy(&backend_body),
                }),
            )
            .await?;
        return Ok(Response::builder()
            .status(backend_status)
            .header(RUN_ID_HEADER, run_id.to_string())
            .header(axum::http::header::CONTENT_TYPE, "application/json")
            .body(Body::from(backend_body))
            .map_err(|e| Error::Internal(e.to_string()))?);
    }

    // 3. Parse result, hash each doc as an artifact
    let result: serde_json::Value = serde_json::from_slice(&backend_body)
        .map_err(|e| Error::Backend(format!("backend returned non-JSON: {e}")))?;

    let docs = result
        .get("docs")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    let mut artifact_ids = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        let doc_bytes =
            serde_json::to_vec(doc).map_err(|e| Error::Internal(e.to_string()))?;
        let label = doc
            .get("id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .unwrap_or_else(|| format!("doc-{i}"));
        let artifact = state
            .ledger
            .upload_artifact(run_id, &label, "application/json", doc_bytes)
            .await?;
        artifact_ids.push(artifact.id);
    }

    // 4. Result event references the artifacts
    state
        .ledger
        .append_event(
            run_id,
            "retrieval_result",
            json!({
                "doc_count": docs.len(),
                "artifact_ids": artifact_ids,
            }),
        )
        .await?;

    // 5. Return the result to the caller unchanged, with run_id header
    Ok(Response::builder()
        .status(backend_status)
        .header(RUN_ID_HEADER, run_id.to_string())
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(backend_body))
        .map_err(|e| Error::Internal(e.to_string()))?)
}
