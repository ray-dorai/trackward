use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use reqwest::multipart;
use serde_json::Value;

/// Spawn a fresh server for each test. Each #[tokio::test] has its own runtime,
/// so we can't share servers across tests — the first runtime dies with the test.
async fn spawn_server() -> String {
    // Load .env so tests pick up S3_ENDPOINT, AWS creds, etc.
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

#[tokio::test]
async fn health_check() {
    let base = spawn_server().await;
    let resp = reqwest::get(format!("{base}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn create_and_get_run() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    // Create a run
    let resp = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({
            "agent": "claude-code-v1",
            "metadata": {"task": "refactor auth"}
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let run: Value = resp.json().await.unwrap();
    let run_id = run["id"].as_str().unwrap();
    assert_eq!(run["agent"], "claude-code-v1");
    assert_eq!(run["metadata"]["task"], "refactor auth");

    // Get the run back
    let resp = client
        .get(format!("{base}/runs/{run_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let fetched: Value = resp.json().await.unwrap();
    assert_eq!(fetched["id"], run_id);
    assert_eq!(fetched["agent"], "claude-code-v1");
}

#[tokio::test]
async fn list_runs() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    // Unique markers so we can find our runs even with other tests running.
    let marker_a = format!("list-runs-a-{}", uuid::Uuid::now_v7());
    let marker_b = format!("list-runs-b-{}", uuid::Uuid::now_v7());

    client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": &marker_a}))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": &marker_b}))
        .send()
        .await
        .unwrap();

    let runs: Vec<Value> = client
        .get(format!("{base}/runs"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(runs.iter().any(|r| r["agent"] == marker_a));
    assert!(runs.iter().any(|r| r["agent"] == marker_b));
}

#[tokio::test]
async fn append_and_list_events() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    // Create run
    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "test-agent"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    // Append two events
    let e1: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .json(&serde_json::json!({
            "kind": "tool_call",
            "body": {"tool": "bash", "command": "ls"}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(e1["seq"], 1);
    assert_eq!(e1["kind"], "tool_call");

    let e2: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .json(&serde_json::json!({
            "kind": "tool_result",
            "body": {"output": "file1.rs\nfile2.rs"}
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(e2["seq"], 2);

    // List events — should come back in order
    let events: Vec<Value> = client
        .get(format!("{base}/runs/{run_id}/events"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["seq"], 1);
    assert_eq!(events[1]["seq"], 2);
}

#[tokio::test]
async fn upload_and_get_artifact() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    // Create run
    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "test-agent"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    // Upload artifact
    let file_content = b"fn main() { println!(\"hello\"); }";
    let expected_hash = ledger::hash::sha256_hex(file_content);

    let form = multipart::Form::new()
        .text("run_id", run_id.to_string())
        .text("label", "main.rs")
        .text("media_type", "text/x-rust")
        .part(
            "file",
            multipart::Part::bytes(file_content.to_vec()).file_name("main.rs"),
        );

    let artifact: Value = client
        .post(format!("{base}/artifacts"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(artifact["sha256"], expected_hash);
    assert_eq!(artifact["label"], "main.rs");
    assert_eq!(artifact["media_type"], "text/x-rust");
    assert_eq!(artifact["size_bytes"], file_content.len() as i64);

    // Get artifact metadata back
    let artifact_id = artifact["id"].as_str().unwrap();
    let fetched: Value = client
        .get(format!("{base}/artifacts/{artifact_id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["sha256"], expected_hash);

    // List artifacts for the run
    let artifacts: Vec<Value> = client
        .get(format!("{base}/runs/{run_id}/artifacts"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0]["id"], artifact_id);
}

#[tokio::test]
async fn get_nonexistent_run_returns_404() {
    let base = spawn_server().await;
    let fake_id = "00000000-0000-0000-0000-000000000000";
    let resp = reqwest::get(format!("{base}/runs/{fake_id}"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn cannot_update_or_delete_runs() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();

    // Create a run
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "immutable-test"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id: uuid::Uuid = run["id"].as_str().unwrap().parse().unwrap();

    // Try to update — should fail
    let update = sqlx::query("UPDATE runs SET agent = 'changed' WHERE id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on runs must fail");

    // Try to delete — should fail
    let delete = sqlx::query("DELETE FROM runs WHERE id = $1")
        .bind(run_id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on runs must fail");
}

#[tokio::test]
async fn cannot_update_or_delete_events() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();

    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "event-immutable"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    let event: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .json(&serde_json::json!({"kind": "noop", "body": {}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let event_id: uuid::Uuid = event["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE events SET kind = 'tampered' WHERE id = $1")
        .bind(event_id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on events must fail");

    let delete = sqlx::query("DELETE FROM events WHERE id = $1")
        .bind(event_id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on events must fail");
}

#[tokio::test]
async fn cannot_update_or_delete_artifacts() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();

    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "art-immutable"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    let form = multipart::Form::new()
        .text("run_id", run_id.to_string())
        .part("file", multipart::Part::bytes(b"data".to_vec()).file_name("x"));

    let artifact: Value = client
        .post(format!("{base}/artifacts"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let artifact_id: uuid::Uuid = artifact["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE artifacts SET sha256 = 'fake' WHERE id = $1")
        .bind(artifact_id)
        .execute(&pool)
        .await;
    assert!(update.is_err(), "UPDATE on artifacts must fail");

    let delete = sqlx::query("DELETE FROM artifacts WHERE id = $1")
        .bind(artifact_id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "DELETE on artifacts must fail");
}

#[tokio::test]
async fn download_artifact_verifies_hash() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "download-test"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    let content = b"Round-tripped artifact bytes.";
    let expected = ledger::hash::sha256_hex(content);

    let form = multipart::Form::new()
        .text("run_id", run_id.to_string())
        .text("label", "roundtrip.bin")
        .part(
            "file",
            multipart::Part::bytes(content.to_vec()).file_name("roundtrip.bin"),
        );
    let artifact: Value = client
        .post(format!("{base}/artifacts"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = artifact["id"].as_str().unwrap();

    // Download and verify the bytes come back unchanged
    let resp = client
        .get(format!("{base}/artifacts/{id}/download"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-sha256").unwrap().to_str().unwrap(),
        expected
    );
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), content);
    assert_eq!(ledger::hash::sha256_hex(&body), expected);
}

#[tokio::test]
async fn append_event_to_missing_run_returns_404() {
    let base = spawn_server().await;
    let fake_run = "00000000-0000-0000-0000-000000000000";
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs/{fake_run}/events"))
        .json(&serde_json::json!({"kind": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn concurrent_appends_produce_monotonic_sequence() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "concurrent-test"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();

    // Fire 20 concurrent appends
    let mut handles = Vec::new();
    for i in 0..20 {
        let url = format!("{base}/runs/{run_id}/events");
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            c.post(&url)
                .json(&serde_json::json!({"kind": "concurrent", "body": {"i": i}}))
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }));
    }
    for h in handles {
        h.await.unwrap();
    }

    // Read back — sequence numbers must be 1..=20 with no gaps or dupes
    let events: Vec<Value> = client
        .get(format!("{base}/runs/{run_id}/events"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(events.len(), 20);
    let seqs: Vec<i64> = events.iter().map(|e| e["seq"].as_i64().unwrap()).collect();
    for (i, &s) in seqs.iter().enumerate() {
        assert_eq!(s, (i as i64) + 1, "gap or duplicate in sequence: {seqs:?}");
    }
}

#[tokio::test]
async fn identical_content_produces_identical_hash() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({"agent": "dedup-test"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    let content = b"deduplicated content";
    let upload = |label: &'static str| {
        let run_id = run_id.to_string();
        let base = base.clone();
        let client = client.clone();
        async move {
            let form = multipart::Form::new()
                .text("run_id", run_id)
                .text("label", label)
                .part("file", multipart::Part::bytes(content.to_vec()).file_name(label));
            client
                .post(format!("{base}/artifacts"))
                .multipart(form)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };

    let a1 = upload("first.txt").await;
    let a2 = upload("second.txt").await;

    // Different artifact rows, different labels, same content-addressed hash
    assert_ne!(a1["id"], a2["id"]);
    assert_ne!(a1["label"], a2["label"]);
    assert_eq!(a1["sha256"], a2["sha256"]);
}

#[tokio::test]
async fn hash_is_deterministic() {
    let data = b"some artifact content";
    let h1 = ledger::hash::sha256_hex(data);
    let h2 = ledger::hash::sha256_hex(data);
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
}
