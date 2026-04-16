mod common;

use common::*;
use std::collections::HashMap;
use serde_json::json;

#[tokio::test]
async fn tool_call_auto_creates_run_and_records_call_and_result() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, backend) =
        spawn_mock_backend(json!({"stdout": "file1\nfile2", "exit": 0})).await;

    let mut routes = HashMap::new();
    routes.insert("bash".into(), backend_url);

    let gw = spawn_gateway(ledger_url.clone(), routes, None, vec![]).await;

    // Act: caller hits the gateway with no run_id header
    let resp = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw.url))
        .json(&json!({"command": "ls"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let run_id = resp
        .headers()
        .get("x-trackward-run-id")
        .expect("gateway must return run_id it created")
        .to_str()
        .unwrap()
        .to_string();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["stdout"], "file1\nfile2");

    // Backend received the caller's payload untouched
    let calls = backend.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["command"], "ls");
    drop(calls);

    // Ledger has exactly tool_call then tool_result, both for this run
    let events = get_events(&ledger_url, &run_id).await;
    assert_eq!(events.len(), 2, "expected tool_call + tool_result, got {events:?}");
    assert_eq!(events[0]["kind"], "tool_call");
    assert_eq!(events[0]["body"]["tool"], "bash");
    assert_eq!(events[0]["body"]["input"]["command"], "ls");
    assert_eq!(events[1]["kind"], "tool_result");
    assert_eq!(events[1]["body"]["output"]["stdout"], "file1\nfile2");
}

#[tokio::test]
async fn tool_call_respects_explicit_run_id() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, _backend) = spawn_mock_backend(json!({"ok": true})).await;

    let mut routes = HashMap::new();
    routes.insert("deploy".into(), backend_url);

    let gw = spawn_gateway(ledger_url.clone(), routes, None, vec![]).await;

    // Caller creates a run directly on the ledger first
    let run: serde_json::Value = reqwest::Client::new()
        .post(format!("{ledger_url}/runs"))
        .json(&json!({"agent": "claude-code"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();

    // Call the gateway with that run_id
    let resp = reqwest::Client::new()
        .post(format!("{}/tool/deploy", gw.url))
        .header("x-trackward-run-id", &run_id)
        .json(&json!({"target": "prod"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-trackward-run-id").unwrap().to_str().unwrap(),
        run_id,
        "gateway must echo back the explicit run_id"
    );

    let events = get_events(&ledger_url, &run_id).await;
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn unknown_tool_returns_404_and_no_events() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url.clone(), HashMap::new(), None, vec![]).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/nonexistent", gw.url))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
    // No run_id returned because the call never reached the ledger
    assert!(resp.headers().get("x-trackward-run-id").is_none());
}

#[tokio::test]
async fn backend_error_is_recorded_as_tool_error_event() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, backend) = spawn_mock_backend(json!({"error": "backend exploded"})).await;
    backend.set_status(500).await;

    let mut routes = HashMap::new();
    routes.insert("flaky".into(), backend_url);

    let gw = spawn_gateway(ledger_url.clone(), routes, None, vec![]).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/flaky", gw.url))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    // Gateway surfaces backend failure — caller sees a 502-ish status
    assert!(resp.status().is_server_error(), "got {}", resp.status());
    let run_id = resp
        .headers()
        .get("x-trackward-run-id")
        .expect("even failed calls must be tied to a run")
        .to_str()
        .unwrap()
        .to_string();

    let events = get_events(&ledger_url, &run_id).await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "tool_call");
    assert_eq!(events[1]["kind"], "tool_error");
    assert_eq!(events[1]["body"]["status"], 500);
}

#[tokio::test]
async fn health_check() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url, HashMap::new(), None, vec![]).await;
    let resp = reqwest::get(format!("{}/health", gw.url)).await.unwrap();
    assert_eq!(resp.status(), 200);
}
