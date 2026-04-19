//! Phase 8c: every append-only write carries a caller identity.
//!
//! Two modes matter:
//!
//! * **Strict** (`default_actor = None`) — production. Every write must
//!   carry an `X-Trackward-Actor` header; missing or malformed header is
//!   400. This is what protects the ledger from an anonymous caller
//!   silently filling rows with `'legacy'`.
//! * **Permissive** (`default_actor = Some(_)`) — local/test/bootstrap.
//!   Missing header falls back to the configured value. An explicit
//!   header still overrides the fallback, so a caller that knows its
//!   identity always wins over the default.
//!
//! The contract applies uniformly across every write endpoint. We spot-
//! check three different tables (runs, events, tool_invocations) to
//! prove the extractor is wired in consistently and not just on one
//! route.

use ledger::actor::{ACTOR_HEADER, MAX_ACTOR_LEN};
use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use serde_json::{json, Value};

async fn spawn_server_with_default(default_actor: Option<String>) -> String {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let blob_store = BlobStore::new(&config).await;
    let state = AppState::new(pool, blob_store).with_default_actor(default_actor);
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn spawn_strict_server() -> String {
    spawn_server_with_default(None).await
}

// ---------------- strict mode: missing/invalid headers are 400 ----------------

#[tokio::test]
async fn create_run_without_header_in_strict_mode_returns_400() {
    let base = spawn_strict_server().await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&json!({"agent": "nobody"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
    let body: Value = resp.json().await.unwrap();
    assert!(
        body["error"].as_str().unwrap_or("").contains("X-Trackward-Actor"),
        "expected error to mention header; got {body:?}"
    );
}

#[tokio::test]
async fn empty_header_value_is_rejected() {
    let base = spawn_strict_server().await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .header(ACTOR_HEADER, "   ") // whitespace-only trims to empty
        .json(&json!({"agent": "nobody"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn oversized_header_value_is_rejected() {
    let base = spawn_strict_server().await;
    let huge = "a".repeat(MAX_ACTOR_LEN + 1);
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .header(ACTOR_HEADER, huge)
        .json(&json!({"agent": "nobody"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}

// ---------------- permissive mode: default_actor fills in ----------------

#[tokio::test]
async fn missing_header_falls_back_to_default_actor() {
    let base = spawn_server_with_default(Some("fallback-actor".into())).await;
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&json!({"agent": "phase8-default"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(run["actor_id"], "fallback-actor");
}

#[tokio::test]
async fn explicit_header_overrides_default_actor() {
    // Even in permissive mode, a caller that knows who they are wins
    // over the configured fallback — the default is a floor, not a cap.
    let base = spawn_server_with_default(Some("fallback-actor".into())).await;
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .header(ACTOR_HEADER, "ops@acme.example")
        .json(&json!({"agent": "phase8-explicit"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(run["actor_id"], "ops@acme.example");
}

#[tokio::test]
async fn header_is_trimmed_before_persisting() {
    let base = spawn_strict_server().await;
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .header(ACTOR_HEADER, "  surrounding-whitespace  ")
        .json(&json!({"agent": "phase8-trim"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(run["actor_id"], "surrounding-whitespace");
}

// ---------------- contract applies uniformly across write endpoints ----------------

#[tokio::test]
async fn events_append_also_requires_actor_in_strict_mode() {
    // Seed a run under a permissive ledger so we have a run_id to target.
    let seed = spawn_server_with_default(Some("seed".into())).await;
    let run: Value = reqwest::Client::new()
        .post(format!("{seed}/runs"))
        .json(&json!({"agent": "phase8-events"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    // Now hit a strict server and confirm the events write also rejects.
    let strict = spawn_strict_server().await;
    let resp = reqwest::Client::new()
        .post(format!("{strict}/runs/{run_id}/events"))
        .json(&json!({"kind": "tool.call", "body": {}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 400);
}

#[tokio::test]
async fn actor_stamped_on_runs_events_and_tool_invocations() {
    // Spot-check three different tables in one test so a reader can see
    // the extractor is wired end-to-end on every write kind, not just /runs.
    let base = spawn_strict_server().await;
    let client = reqwest::Client::new();
    let actor = "integration-bot";

    let run: Value = client
        .post(format!("{base}/runs"))
        .header(ACTOR_HEADER, actor)
        .json(&json!({"agent": "phase8-xwrite"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();
    assert_eq!(run["actor_id"], actor);

    let event: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .header(ACTOR_HEADER, actor)
        .json(&json!({"kind": "tool.call", "body": {"tool": "echo"}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(event["actor_id"], actor);

    let tool_inv: Value = client
        .post(format!("{base}/tool-invocations"))
        .header(ACTOR_HEADER, actor)
        .json(&json!({
            "run_id": run_id,
            "tool": "echo",
            "status": "ok",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tool_inv["actor_id"], actor);
}

#[tokio::test]
async fn different_callers_stamp_different_actors_on_same_run() {
    // Two distinct callers appending to the same run produce two events
    // with distinct actor_ids. This is the property that lets auditors
    // reconstruct who did what across a multi-actor run.
    let base = spawn_strict_server().await;
    let client = reqwest::Client::new();

    let run: Value = client
        .post(format!("{base}/runs"))
        .header(ACTOR_HEADER, "orchestrator")
        .json(&json!({"agent": "phase8-multiactor"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    let first: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .header(ACTOR_HEADER, "agent-alpha")
        .json(&json!({"kind": "reason", "body": {}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .header(ACTOR_HEADER, "agent-beta")
        .json(&json!({"kind": "reason", "body": {}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(first["actor_id"], "agent-alpha");
    assert_eq!(second["actor_id"], "agent-beta");
    assert_ne!(first["id"], second["id"]);
}
