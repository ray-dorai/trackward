//! Phase 3: registry/index tables — prompt_versions, policy_versions,
//! eval_results, run_version_bindings. All append-only; every run recorded by
//! the gateway eventually carries references to these rows.

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

#[tokio::test]
async fn create_and_get_prompt_version() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let body = json!({
        "workflow": "example-workflow",
        "version": "1.0.0",
        "git_sha": "deadbeefcafebabe0000000000000000deadbeef",
        "content_hash": "a".repeat(64),
        "metadata": {"author": "ray"}
    });

    let created: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["workflow"], "example-workflow");
    assert_eq!(created["version"], "1.0.0");
    assert_eq!(created["git_sha"], body["git_sha"]);
    assert_eq!(created["content_hash"], body["content_hash"]);
    let id = created["id"].as_str().unwrap();

    let fetched: Value = client
        .get(format!("{base}/prompt-versions/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["id"], id);
}

#[tokio::test]
async fn lookup_prompt_version_by_workflow_and_hash() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let hash = "b".repeat(64);
    let created: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "lookup-test",
            "version": "1.2.3",
            "git_sha": "0123456789abcdef0123456789abcdef01234567",
            "content_hash": hash,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap();

    let matches: Vec<Value> = client
        .get(format!(
            "{base}/prompt-versions?workflow=lookup-test&version=1.2.3"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(matches.iter().any(|m| m["id"] == id));
}

#[tokio::test]
async fn create_and_get_policy_version() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let created: Value = client
        .post(format!("{base}/policy-versions"))
        .json(&json!({
            "scope": "global",
            "version": "1.0.0",
            "git_sha": "feedfacefeedfacefeedfacefeedfacefeedface",
            "content_hash": "c".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["scope"], "global");
    let id = created["id"].as_str().unwrap();

    let fetched: Value = client
        .get(format!("{base}/policy-versions/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["id"], id);
}

#[tokio::test]
async fn create_eval_result_linked_to_prompt() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let prompt: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "eval-link",
            "version": "1.0.0",
            "git_sha": "1".repeat(40),
            "content_hash": "d".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let prompt_id = prompt["id"].as_str().unwrap();

    let eval: Value = client
        .post(format!("{base}/eval-results"))
        .json(&json!({
            "workflow": "eval-link",
            "version": "1.0.0",
            "prompt_version_id": prompt_id,
            "git_sha": "2".repeat(40),
            "content_hash": "e".repeat(64),
            "passed": true,
            "summary": {"happy_path": 10, "refusal": 3}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(eval["passed"], true);
    assert_eq!(eval["prompt_version_id"], prompt_id);
    assert_eq!(eval["summary"]["happy_path"], 10);
}

#[tokio::test]
async fn bind_run_to_versions() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let run_id = create_run(&base, "binding-test").await;

    let prompt: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "bind-test",
            "version": "1.0.0",
            "git_sha": "3".repeat(40),
            "content_hash": "0".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let policy: Value = client
        .post(format!("{base}/policy-versions"))
        .json(&json!({
            "scope": "global",
            "version": "1.0.0",
            "git_sha": "4".repeat(40),
            "content_hash": "1".repeat(64),
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
            "workflow": "bind-test",
            "version": "1.0.0",
            "prompt_version_id": prompt["id"],
            "git_sha": "5".repeat(40),
            "content_hash": "2".repeat(64),
            "passed": true,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let binding: Value = client
        .post(format!("{base}/runs/{run_id}/bindings"))
        .json(&json!({
            "prompt_version_id": prompt["id"],
            "policy_version_id": policy["id"],
            "eval_result_id": eval["id"],
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(binding["run_id"], run_id);
    assert_eq!(binding["prompt_version_id"], prompt["id"]);
    assert_eq!(binding["policy_version_id"], policy["id"]);
    assert_eq!(binding["eval_result_id"], eval["id"]);

    // And: reading it back gives the same row
    let fetched: Value = client
        .get(format!("{base}/runs/{run_id}/bindings"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["run_id"], run_id);
    assert_eq!(fetched["prompt_version_id"], prompt["id"]);
}

#[tokio::test]
async fn cannot_double_bind_a_run() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let run_id = create_run(&base, "double-bind").await;

    let prompt: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "double-bind",
            "version": "1.0.0",
            "git_sha": "6".repeat(40),
            "content_hash": "3".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let first = client
        .post(format!("{base}/runs/{run_id}/bindings"))
        .json(&json!({"prompt_version_id": prompt["id"]}))
        .send()
        .await
        .unwrap();
    assert!(first.status().is_success());

    // Second bind must fail — bindings are append-only, one per run.
    let second = client
        .post(format!("{base}/runs/{run_id}/bindings"))
        .json(&json!({"prompt_version_id": prompt["id"]}))
        .send()
        .await
        .unwrap();
    assert!(
        !second.status().is_success(),
        "second binding must be rejected, got {}",
        second.status()
    );
}

#[tokio::test]
async fn cannot_update_or_delete_prompt_versions() {
    let base = spawn_server().await;
    let _ = base; // ensure migrations run
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();

    let client = reqwest::Client::new();
    let created: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "immutable",
            "version": "1.0.0",
            "git_sha": "7".repeat(40),
            "content_hash": "4".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE prompt_versions SET workflow = 'x' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on prompt_versions must fail");

    let delete = sqlx::query("DELETE FROM prompt_versions WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on prompt_versions must fail");
}

#[tokio::test]
async fn cannot_update_or_delete_run_bindings() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();

    let run_id = create_run(&base, "binding-immutable").await;
    let prompt: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "binding-immutable",
            "version": "1.0.0",
            "git_sha": "8".repeat(40),
            "content_hash": "5".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    client
        .post(format!("{base}/runs/{run_id}/bindings"))
        .json(&json!({"prompt_version_id": prompt["id"]}))
        .send()
        .await
        .unwrap();

    let run_uuid: uuid::Uuid = run_id.parse().unwrap();
    let update = sqlx::query(
        "UPDATE run_version_bindings SET prompt_version_id = NULL WHERE run_id = $1",
    )
    .bind(run_uuid)
    .execute(&pool)
    .await;
    assert!(update.is_err(), "UPDATE on run_version_bindings must fail");

    let delete = sqlx::query("DELETE FROM run_version_bindings WHERE run_id = $1")
        .bind(run_uuid)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on run_version_bindings must fail");
}

#[tokio::test]
async fn cannot_update_or_delete_policy_versions() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();

    let created: Value = client
        .post(format!("{base}/policy-versions"))
        .json(&json!({
            "scope": "immutable-policy",
            "version": "1.0.0",
            "git_sha": "9".repeat(40),
            "content_hash": "6".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE policy_versions SET scope = 'x' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on policy_versions must fail");

    let delete = sqlx::query("DELETE FROM policy_versions WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on policy_versions must fail");
}

#[tokio::test]
async fn cannot_update_or_delete_eval_results() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();

    let prompt: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "eval-immutable",
            "version": "1.0.0",
            "git_sha": "a".repeat(40),
            "content_hash": "7".repeat(64),
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let created: Value = client
        .post(format!("{base}/eval-results"))
        .json(&json!({
            "workflow": "eval-immutable",
            "version": "1.0.0",
            "prompt_version_id": prompt["id"],
            "git_sha": "b".repeat(40),
            "content_hash": "8".repeat(64),
            "passed": true,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE eval_results SET passed = false WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on eval_results must fail");

    let delete = sqlx::query("DELETE FROM eval_results WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on eval_results must fail");
}

#[tokio::test]
async fn reregister_same_prompt_version_returns_existing_row() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let body = json!({
        "workflow": "idempotent-test",
        "version": "1.0.0",
        "git_sha": "c".repeat(40),
        "content_hash": "9".repeat(64),
    });

    let first: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let second: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&body)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        first["id"], second["id"],
        "re-registering identical (workflow, version, content_hash) must return the same row"
    );
}
