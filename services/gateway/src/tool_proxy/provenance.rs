use axum::http::HeaderMap;
use uuid::Uuid;

use crate::AppState;

pub const RUN_ID_HEADER: &str = "x-trackward-run-id";

/// Extract a run_id from request headers, or None.
pub fn extract_run_id(headers: &HeaderMap) -> Option<Uuid> {
    headers
        .get(RUN_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

/// Mint a new run on the ledger if the caller didn't provide one, and stamp
/// the cached registry binding onto it. If the caller *did* provide a run_id,
/// we assume they own the binding and leave it alone.
///
/// Returns `(run_id, minted)` so handlers can log/react to which case they hit.
pub async fn resolve_or_mint_run(
    state: &AppState,
    headers: &HeaderMap,
    origin: &str,
) -> Result<(Uuid, bool), crate::errors::Error> {
    if let Some(id) = extract_run_id(headers) {
        return Ok((id, false));
    }
    let run_id = state
        .ledger
        .create_run("gateway", serde_json::json!({"origin": origin}))
        .await?;

    // Stamp the run with the active registry binding. If nothing is bound
    // (empty config, or resolve failed at startup), skip — a run with no
    // binding is still a valid run, just one we can't trace back to a
    // specific prompt/policy.
    if !state.binding.is_empty() {
        if let Err(e) = state
            .ledger
            .bind_run(
                run_id,
                state.binding.prompt_version_id,
                state.binding.policy_version_id,
                state.binding.eval_result_id,
            )
            .await
        {
            // Don't fail the caller's request if the bind fails — the run is
            // already created and the tool_call event still gets recorded.
            // But do log loudly so operators notice.
            tracing::error!(error = %e, %run_id, "failed to stamp run with binding");
        }
    }

    Ok((run_id, true))
}
