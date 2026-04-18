//! Phase 6: haruspex read surface.
//!
//! An investigator needs to (a) find the run they're looking for among
//! hundreds, and (b) pull *everything* that happened during that run into
//! one payload. Two endpoints do that job: a filtered `GET /runs` and a
//! `GET /runs/:id/dossier` that joins every evidence table for one run.
//! For case-level work, `GET /cases/:id/dossier` resolves each
//! `case_evidence` link into the underlying row (so a UI doesn't need to
//! do N round-trips).

use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use serde_json::{json, Value};

async fn spawn_server() -> String {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let blob_store = BlobStore::new(&config).await;
    let state = AppState::new(pool, blob_store);
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn create_run(base: &str, agent: &str, started_at: Option<&str>) -> String {
    let mut body = json!({"agent": agent});
    if let Some(t) = started_at {
        body["started_at"] = json!(t);
    }
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    run["id"].as_str().unwrap().to_string()
}

// ------------------------------ filtered list -----------------------------

#[tokio::test]
async fn list_runs_filters_by_agent() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let marker = format!("phase6-filter-{}", uuid::Uuid::now_v7());
    let want_agent = format!("{marker}-A");
    let other_agent = format!("{marker}-B");
    create_run(&base, &want_agent, None).await;
    create_run(&base, &want_agent, None).await;
    create_run(&base, &other_agent, None).await;

    let rows: Vec<Value> = client
        .get(format!("{base}/runs?agent={want_agent}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "got {rows:?}");
    for r in &rows {
        assert_eq!(r["agent"], want_agent);
    }
}

#[tokio::test]
async fn list_runs_filters_by_time_window() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let agent = format!("phase6-window-{}", uuid::Uuid::now_v7());
    create_run(&base, &agent, Some("2026-01-01T00:00:00Z")).await;
    let mid = create_run(&base, &agent, Some("2026-02-15T00:00:00Z")).await;
    create_run(&base, &agent, Some("2026-03-30T00:00:00Z")).await;

    let rows: Vec<Value> = client
        .get(format!(
            "{base}/runs?agent={agent}\
             &since=2026-02-01T00:00:00Z\
             &until=2026-03-01T00:00:00Z"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "got {rows:?}");
    assert_eq!(rows[0]["id"], mid);
}

#[tokio::test]
async fn list_runs_respects_limit() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let agent = format!("phase6-limit-{}", uuid::Uuid::now_v7());
    for _ in 0..5 {
        create_run(&base, &agent, None).await;
    }
    let rows: Vec<Value> = client
        .get(format!("{base}/runs?agent={agent}&limit=2"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2, "got {rows:?}");
}

// ------------------------------ run dossier -----------------------------

#[tokio::test]
async fn run_dossier_includes_all_evidence() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "dossier-agent", None).await;

    // Append an event.
    client
        .post(format!("{base}/runs/{run_id}/events"))
        .json(&json!({"kind": "tool.call", "payload": {"tool": "echo"}}))
        .send()
        .await
        .unwrap();

    // Tool invocation.
    let tool_inv: Value = client
        .post(format!("{base}/tool-invocations"))
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

    // Side effect tied to that invocation.
    client
        .post(format!("{base}/side-effects"))
        .json(&json!({
            "run_id": run_id,
            "tool_invocation_id": tool_inv["id"],
            "kind": "email",
            "target": "foo@example.com",
            "status": "sent",
        }))
        .send()
        .await
        .unwrap();

    // Guardrail event.
    client
        .post(format!("{base}/guardrails"))
        .json(&json!({
            "run_id": run_id,
            "policy": "pii",
            "decision": "allow",
        }))
        .send()
        .await
        .unwrap();

    // Human approval.
    let approval_id = uuid::Uuid::now_v7();
    client
        .post(format!("{base}/human-approvals"))
        .json(&json!({
            "id": approval_id,
            "run_id": run_id,
            "tool": "wire",
            "decision": "granted",
            "requested_at": "2026-04-18T00:00:00Z",
        }))
        .send()
        .await
        .unwrap();

    // Dossier collects everything.
    let dossier: Value = client
        .get(format!("{base}/runs/{run_id}/dossier"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(dossier["run"]["id"], run_id);
    for key in [
        "events",
        "tool_invocations",
        "side_effects",
        "guardrails",
        "human_approvals",
        "bias_slices",
        "artifacts",
    ] {
        assert!(
            dossier.get(key).is_some() && dossier[key].is_array(),
            "dossier missing array field {key}; got {dossier:?}"
        );
    }
    assert_eq!(dossier["events"].as_array().unwrap().len(), 1);
    assert_eq!(dossier["tool_invocations"].as_array().unwrap().len(), 1);
    assert_eq!(dossier["side_effects"].as_array().unwrap().len(), 1);
    assert_eq!(dossier["guardrails"].as_array().unwrap().len(), 1);
    assert_eq!(dossier["human_approvals"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn run_dossier_for_missing_run_is_404() {
    let base = spawn_server().await;
    let missing = uuid::Uuid::now_v7();
    let resp = reqwest::Client::new()
        .get(format!("{base}/runs/{missing}/dossier"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

// ------------------------------ case dossier -----------------------------

#[tokio::test]
async fn case_dossier_resolves_linked_evidence() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let run_id = create_run(&base, "case-dossier-agent", None).await;
    let case: Value = client
        .post(format!("{base}/cases"))
        .json(&json!({"title": "case-dossier", "opened_by": "ray"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let case_id = case["id"].as_str().unwrap();

    client
        .post(format!("{base}/cases/{case_id}/evidence"))
        .json(&json!({
            "evidence_type": "run",
            "evidence_id": run_id,
            "linked_by": "ray",
        }))
        .send()
        .await
        .unwrap();

    let dossier: Value = client
        .get(format!("{base}/cases/{case_id}/dossier"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(dossier["case"]["id"], case_id);
    let evidence = dossier["evidence"].as_array().unwrap();
    assert_eq!(evidence.len(), 1);
    let entry = &evidence[0];
    // Each entry: the link row + the resolved underlying row.
    assert_eq!(entry["link"]["evidence_type"], "run");
    assert_eq!(entry["link"]["evidence_id"], run_id);
    assert_eq!(entry["resolved"]["id"], run_id);
    assert_eq!(entry["resolved"]["agent"], "case-dossier-agent");
}

#[tokio::test]
async fn case_dossier_handles_unresolvable_evidence_type() {
    // Forward-compatibility: if a case links evidence of a type the
    // dossier doesn't know how to resolve, the link still shows up —
    // just with resolved:null. We don't want the whole dossier to 500
    // because someone added a new evidence type.
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let case: Value = client
        .post(format!("{base}/cases"))
        .json(&json!({"title": "mystery-case", "opened_by": "ray"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let case_id = case["id"].as_str().unwrap();
    let mystery_id = uuid::Uuid::now_v7();
    client
        .post(format!("{base}/cases/{case_id}/evidence"))
        .json(&json!({
            "evidence_type": "from-the-future",
            "evidence_id": mystery_id,
            "linked_by": "ray",
        }))
        .send()
        .await
        .unwrap();

    let dossier: Value = client
        .get(format!("{base}/cases/{case_id}/dossier"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let evidence = dossier["evidence"].as_array().unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0]["link"]["evidence_type"], "from-the-future");
    assert!(evidence[0]["resolved"].is_null());
}

#[tokio::test]
async fn case_dossier_for_missing_case_is_404() {
    let base = spawn_server().await;
    let missing = uuid::Uuid::now_v7();
    let resp = reqwest::Client::new()
        .get(format!("{base}/cases/{missing}/dossier"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}
