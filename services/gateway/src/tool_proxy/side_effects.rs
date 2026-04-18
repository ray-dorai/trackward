//! Downstream confirmation recording.
//!
//! After a tool invocation returns, the backend may include a `side_effects`
//! array describing observable changes it caused in other systems (DB writes,
//! HTTP POSTs, emails, file writes). Each entry is split into its own
//! `side_effect` ledger row, linked back to the originating tool_invocation.
//!
//! Shape expected from the backend:
//! ```json
//! {
//!   "side_effects": [
//!     {"kind": "http", "target": "https://...", "status": "confirmed",
//!      "confirmation": {"http_status": 200}}
//!   ]
//! }
//! ```
//! Missing fields default to `"unknown"` / `{}`. A body without a
//! `side_effects` array is a no-op — most tools produce nothing to record.

use uuid::Uuid;

use crate::ledger_client::LedgerClient;

/// Record any side effects the backend declared in its response body. Errors
/// on individual rows are logged and swallowed — a broken downstream write
/// shouldn't mask the tool's main result from the caller.
pub async fn record_confirmations(
    ledger: &LedgerClient,
    run_id: Uuid,
    tool_invocation_id: Option<Uuid>,
    body: &serde_json::Value,
) {
    let Some(entries) = body.get("side_effects").and_then(|v| v.as_array()) else {
        return;
    };

    for entry in entries {
        let kind = entry
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let target = entry
            .get("target")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let status = entry
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("confirmed");
        let confirmation = entry
            .get("confirmation")
            .cloned()
            .unwrap_or(serde_json::json!({}));

        if let Err(e) = ledger
            .record_side_effect(run_id, tool_invocation_id, kind, target, status, &confirmation)
            .await
        {
            tracing::warn!(
                error = %e,
                run_id = %run_id,
                kind,
                target,
                "failed to record side_effect"
            );
        }
    }
}
