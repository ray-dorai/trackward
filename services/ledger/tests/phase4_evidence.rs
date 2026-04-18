//! Phase 4: first-class evidence records — tool_invocations, side_effects,
//! guardrails, human_approvals, bias_slices. Every row is append-only; routes
//! follow the same shape as Phase 3 (POST to create, GET by id, run-scoped list).

use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use serde_json::{json, Value};

async fn spawn_server() -> String {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = Config::from_env();
    let pool = ledger::db::connect(&config)
        .await
        .expect("failed to connect — is docker-compose up?");
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

async fn create_run(base: &str, agent: &str) -> String {
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&json!({"agent": agent}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    run["id"].as_str().unwrap().to_string()
}

// ------------------------------ tool_invocations ----------------------------

#[tokio::test]
async fn create_and_get_tool_invocation() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "ti-basic").await;

    let body = json!({
        "run_id": run_id,
        "tool": "bash",
        "input": {"command": "ls"},
        "output": {"stdout": "ok"},
        "status": "ok",
        "status_code": 200,
    });
    let created: Value = client
        .post(format!("{base}/tool-invocations"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["run_id"], run_id);
    assert_eq!(created["tool"], "bash");
    assert_eq!(created["status"], "ok");
    assert_eq!(created["status_code"], 200);
    let id = created["id"].as_str().unwrap();

    let fetched: Value = client
        .get(format!("{base}/tool-invocations/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["id"], id);
    assert_eq!(fetched["input"]["command"], "ls");
}

#[tokio::test]
async fn list_tool_invocations_by_run() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "ti-list").await;

    for tool in ["bash", "deploy"] {
        client
            .post(format!("{base}/tool-invocations"))
            .json(&json!({"run_id": run_id, "tool": tool, "status": "ok"}))
            .send()
            .await
            .unwrap();
    }

    let rows: Vec<Value> = client
        .get(format!("{base}/runs/{run_id}/tool-invocations"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    let tools: Vec<&str> = rows.iter().map(|r| r["tool"].as_str().unwrap()).collect();
    assert!(tools.contains(&"bash"));
    assert!(tools.contains(&"deploy"));
}

#[tokio::test]
async fn cannot_update_or_delete_tool_invocations() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "ti-immutable").await;

    let created: Value = client
        .post(format!("{base}/tool-invocations"))
        .json(&json!({"run_id": run_id, "tool": "x", "status": "ok"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE tool_invocations SET status = 'error' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on tool_invocations must fail");
    let delete = sqlx::query("DELETE FROM tool_invocations WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on tool_invocations must fail");
}

// ------------------------------ side_effects -------------------------------

#[tokio::test]
async fn create_side_effect_linked_to_tool_invocation() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "se-basic").await;

    let ti: Value = client
        .post(format!("{base}/tool-invocations"))
        .json(&json!({"run_id": run_id, "tool": "deploy", "status": "ok"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ti_id = ti["id"].as_str().unwrap();

    let se: Value = client
        .post(format!("{base}/side-effects"))
        .json(&json!({
            "run_id": run_id,
            "tool_invocation_id": ti_id,
            "kind": "http",
            "target": "https://api.example.com/deploy",
            "status": "confirmed",
            "confirmation": {"http_status": 200, "deployment_id": "dep-123"}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(se["run_id"], run_id);
    assert_eq!(se["tool_invocation_id"], ti_id);
    assert_eq!(se["kind"], "http");
    assert_eq!(se["status"], "confirmed");
    assert_eq!(se["confirmation"]["deployment_id"], "dep-123");
}

#[tokio::test]
async fn list_side_effects_by_run() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "se-list").await;

    for target in ["db://users", "s3://bucket/key"] {
        client
            .post(format!("{base}/side-effects"))
            .json(&json!({
                "run_id": run_id,
                "kind": "write",
                "target": target,
                "status": "confirmed",
            }))
            .send()
            .await
            .unwrap();
    }

    let rows: Vec<Value> = client
        .get(format!("{base}/runs/{run_id}/side-effects"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn cannot_update_or_delete_side_effects() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "se-immutable").await;

    let created: Value = client
        .post(format!("{base}/side-effects"))
        .json(&json!({
            "run_id": run_id,
            "kind": "http",
            "target": "x",
            "status": "confirmed",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE side_effects SET status = 'failed' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on side_effects must fail");
    let delete = sqlx::query("DELETE FROM side_effects WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on side_effects must fail");
}

// ------------------------------ guardrails ---------------------------------

#[tokio::test]
async fn create_and_list_guardrail() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "gr-basic").await;

    let created: Value = client
        .post(format!("{base}/guardrails"))
        .json(&json!({
            "run_id": run_id,
            "name": "tool_allowlist",
            "stage": "pre_tool",
            "target": "rm",
            "outcome": "blocked",
            "detail": {"allowed": ["bash", "deploy"]}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["name"], "tool_allowlist");
    assert_eq!(created["outcome"], "blocked");

    let rows: Vec<Value> = client
        .get(format!("{base}/runs/{run_id}/guardrails"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["target"], "rm");
}

#[tokio::test]
async fn cannot_update_or_delete_guardrails() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "gr-immutable").await;

    let created: Value = client
        .post(format!("{base}/guardrails"))
        .json(&json!({
            "run_id": run_id,
            "name": "rate_limit",
            "stage": "pre_tool",
            "outcome": "passed",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE guardrails SET outcome = 'blocked' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on guardrails must fail");
    let delete = sqlx::query("DELETE FROM guardrails WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on guardrails must fail");
}

// ------------------------------ human_approvals -----------------------------

#[tokio::test]
async fn create_and_get_human_approval() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "ha-basic").await;

    let approval_id = uuid::Uuid::now_v7().to_string();
    let requested_at = "2026-04-18T08:00:00Z";

    let created: Value = client
        .post(format!("{base}/human-approvals"))
        .json(&json!({
            "id": approval_id,
            "run_id": run_id,
            "tool": "deploy",
            "decision": "granted",
            "reason": "looks fine",
            "decided_by": "ray",
            "requested_at": requested_at,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["id"], approval_id);
    assert_eq!(created["decision"], "granted");
    assert_eq!(created["decided_by"], "ray");

    let fetched: Value = client
        .get(format!("{base}/human-approvals/{approval_id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["id"], approval_id);
    assert_eq!(fetched["reason"], "looks fine");
}

#[tokio::test]
async fn cannot_update_or_delete_human_approvals() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "ha-immutable").await;

    let approval_id = uuid::Uuid::now_v7().to_string();
    let created: Value = client
        .post(format!("{base}/human-approvals"))
        .json(&json!({
            "id": approval_id,
            "run_id": run_id,
            "tool": "x",
            "decision": "denied",
            "requested_at": "2026-04-18T08:00:00Z",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE human_approvals SET decision = 'granted' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on human_approvals must fail");
    let delete = sqlx::query("DELETE FROM human_approvals WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on human_approvals must fail");
}

// ------------------------------ bias_slices --------------------------------

#[tokio::test]
async fn bias_slice_can_attach_to_run_or_eval() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "bs-basic").await;

    let run_slice: Value = client
        .post(format!("{base}/bias-slices"))
        .json(&json!({
            "run_id": run_id,
            "label": "region",
            "value": "eu",
            "score": 0.83,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(run_slice["run_id"], run_id);
    assert_eq!(run_slice["label"], "region");
    assert_eq!(run_slice["value"], "eu");

    // Create an eval_result so we can attach a bias slice to it.
    let prompt: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "bias-slice-eval",
            "version": "1.0.0",
            "git_sha": "a".repeat(40),
            "content_hash": "b".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let eval: Value = client
        .post(format!("{base}/eval-results"))
        .json(&json!({
            "workflow": "bias-slice-eval",
            "version": "1.0.0",
            "prompt_version_id": prompt["id"],
            "git_sha": "c".repeat(40),
            "content_hash": "d".repeat(64),
            "passed": true,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let eval_slice: Value = client
        .post(format!("{base}/bias-slices"))
        .json(&json!({
            "eval_result_id": eval["id"],
            "label": "gender",
            "value": "female",
            "score": 0.91,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(eval_slice["eval_result_id"], eval["id"]);
    assert!(eval_slice["run_id"].is_null());
}

#[tokio::test]
async fn bias_slice_requires_run_or_eval() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    // Neither run_id nor eval_result_id — must fail at the DB check constraint.
    let resp = client
        .post(format!("{base}/bias-slices"))
        .json(&json!({"label": "orphan"}))
        .send()
        .await
        .unwrap();
    assert!(
        !resp.status().is_success(),
        "bias slice with no run or eval must be rejected, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn list_bias_slices_by_run() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "bs-list").await;

    for (label, value) in [("region", "eu"), ("region", "na")] {
        client
            .post(format!("{base}/bias-slices"))
            .json(&json!({"run_id": run_id, "label": label, "value": value}))
            .send()
            .await
            .unwrap();
    }

    let rows: Vec<Value> = client
        .get(format!("{base}/runs/{run_id}/bias-slices"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn cannot_update_or_delete_bias_slices() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "bs-immutable").await;

    let created: Value = client
        .post(format!("{base}/bias-slices"))
        .json(&json!({"run_id": run_id, "label": "x"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE bias_slices SET label = 'y' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on bias_slices must fail");
    let delete = sqlx::query("DELETE FROM bias_slices WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on bias_slices must fail");
}
