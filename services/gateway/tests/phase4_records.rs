//! Phase 4 (gateway): the proxy writes a tool_invocation row for every tool
//! call, records side_effects when the backend returns a `side_effects` array,
//! and the approval gate writes a human_approval row on each decision.

mod common;

use common::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

async fn wait_for_tool_invocations(
    ledger_url: &str,
    run_id: &str,
    n: usize,
) -> Vec<Value> {
    for _ in 0..50 {
        let rows: Vec<Value> = reqwest::get(format!(
            "{ledger_url}/runs/{run_id}/tool-invocations"
        ))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        if rows.len() >= n {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {n} tool_invocations on run {run_id}");
}

async fn wait_for_side_effects(ledger_url: &str, run_id: &str, n: usize) -> Vec<Value> {
    for _ in 0..50 {
        let rows: Vec<Value> =
            reqwest::get(format!("{ledger_url}/runs/{run_id}/side-effects"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        if rows.len() >= n {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {n} side_effects on run {run_id}");
}

#[tokio::test]
async fn tool_call_records_tool_invocation_row() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, _backend) =
        spawn_mock_backend(json!({"stdout": "hello", "exit": 0})).await;

    let mut routes = HashMap::new();
    routes.insert("bash".into(), backend_url);
    let gw = spawn_gateway(ledger_url.clone(), routes, None, vec![]).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw.url))
        .json(&json!({"command": "echo hello"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let run_id = resp
        .headers()
        .get("x-trackward-run-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let rows = wait_for_tool_invocations(&ledger_url, &run_id, 1).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["tool"], "bash");
    assert_eq!(rows[0]["status"], "ok");
    assert_eq!(rows[0]["status_code"], 200);
    assert_eq!(rows[0]["input"]["command"], "echo hello");
    assert_eq!(rows[0]["output"]["stdout"], "hello");
}

#[tokio::test]
async fn tool_error_records_tool_invocation_with_error_status() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, backend) =
        spawn_mock_backend(json!({"error": "boom"})).await;
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
    let run_id = resp
        .headers()
        .get("x-trackward-run-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let rows = wait_for_tool_invocations(&ledger_url, &run_id, 1).await;
    assert_eq!(rows[0]["status"], "error");
    assert_eq!(rows[0]["status_code"], 500);
}

#[tokio::test]
async fn backend_side_effects_array_records_side_effect_rows() {
    let ledger_url = spawn_ledger().await;
    // Backend returns two confirmations — a DB write and an HTTP POST.
    let (backend_url, _backend) = spawn_mock_backend(json!({
        "ok": true,
        "side_effects": [
            {
                "kind": "db_write",
                "target": "users/42",
                "status": "confirmed",
                "confirmation": {"rows": 1}
            },
            {
                "kind": "http",
                "target": "https://api.example.com/notify",
                "status": "confirmed",
                "confirmation": {"http_status": 204}
            }
        ]
    }))
    .await;

    let mut routes = HashMap::new();
    routes.insert("deploy".into(), backend_url);
    let gw = spawn_gateway(ledger_url.clone(), routes, None, vec![]).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/deploy", gw.url))
        .json(&json!({"target": "prod"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let run_id = resp
        .headers()
        .get("x-trackward-run-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let ti_rows = wait_for_tool_invocations(&ledger_url, &run_id, 1).await;
    let ti_id = ti_rows[0]["id"].as_str().unwrap();

    let se_rows = wait_for_side_effects(&ledger_url, &run_id, 2).await;
    assert_eq!(se_rows.len(), 2);
    // Both side effects should be linked to the tool_invocation we just made.
    for row in &se_rows {
        assert_eq!(row["tool_invocation_id"], ti_id);
        assert_eq!(row["run_id"], run_id);
    }
    let kinds: Vec<&str> = se_rows
        .iter()
        .map(|r| r["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"db_write"));
    assert!(kinds.contains(&"http"));
}

#[tokio::test]
async fn backend_without_side_effects_records_none() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, _backend) =
        spawn_mock_backend(json!({"stdout": "quiet"})).await;

    let mut routes = HashMap::new();
    routes.insert("bash".into(), backend_url);
    let gw = spawn_gateway(ledger_url.clone(), routes, None, vec![]).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw.url))
        .json(&json!({"command": "true"}))
        .send()
        .await
        .unwrap();
    let run_id = resp
        .headers()
        .get("x-trackward-run-id")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    // Give async bookkeeping a moment to settle.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let se_rows: Vec<Value> =
        reqwest::get(format!("{ledger_url}/runs/{run_id}/side-effects"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert!(
        se_rows.is_empty(),
        "expected no side effects, got {se_rows:?}"
    );
}

#[tokio::test]
async fn approval_decision_records_human_approval_row() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url.clone(), HashMap::new(), None, vec![]).await;

    // Pre-create a run so we know its id.
    let run: Value = reqwest::Client::new()
        .post(format!("{ledger_url}/runs"))
        .json(&json!({"agent": "ha-row-test"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();

    let gw_url = gw.url.clone();
    let run_id_for_req = run_id.clone();
    let req_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{gw_url}/approval/request"))
            .header("x-trackward-run-id", &run_id_for_req)
            .json(&json!({"tool": "deploy", "reason": "ship v3"}))
            .send()
            .await
            .unwrap()
    });

    // Fish out the approval_id via the approval_requested event.
    let mut approval_id: Option<String> = None;
    for _ in 0..50 {
        let events: Vec<Value> = reqwest::get(format!("{ledger_url}/runs/{run_id}/events"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(ev) = events.iter().find(|e| e["kind"] == "approval_requested") {
            approval_id = Some(ev["body"]["approval_id"].as_str().unwrap().to_string());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let approval_id = approval_id.expect("approval_requested event should exist");

    reqwest::Client::new()
        .post(format!("{}/approval/{approval_id}/decide", gw.url))
        .json(&json!({"decision": "granted", "reason": "ok"}))
        .send()
        .await
        .unwrap();

    let resp = tokio::time::timeout(Duration::from_secs(2), req_task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Now: the human_approvals table must have a row keyed by approval_id.
    let row: Value = reqwest::get(format!("{ledger_url}/human-approvals/{approval_id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(row["id"], approval_id);
    assert_eq!(row["run_id"], run_id);
    assert_eq!(row["tool"], "deploy");
    assert_eq!(row["decision"], "granted");
    assert_eq!(row["reason"], "ok");
}

#[tokio::test]
async fn approval_denial_records_human_approval_row() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url.clone(), HashMap::new(), None, vec![]).await;

    let run: Value = reqwest::Client::new()
        .post(format!("{ledger_url}/runs"))
        .json(&json!({"agent": "ha-deny-test"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();

    let gw_url = gw.url.clone();
    let run_id_for_req = run_id.clone();
    let req_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{gw_url}/approval/request"))
            .header("x-trackward-run-id", &run_id_for_req)
            .json(&json!({"tool": "rm", "reason": "cleanup"}))
            .send()
            .await
            .unwrap()
    });

    let mut approval_id: Option<String> = None;
    for _ in 0..50 {
        let events: Vec<Value> = reqwest::get(format!("{ledger_url}/runs/{run_id}/events"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(ev) = events.iter().find(|e| e["kind"] == "approval_requested") {
            approval_id = Some(ev["body"]["approval_id"].as_str().unwrap().to_string());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let approval_id = approval_id.unwrap();

    reqwest::Client::new()
        .post(format!("{}/approval/{approval_id}/decide", gw.url))
        .json(&json!({"decision": "denied", "reason": "too risky"}))
        .send()
        .await
        .unwrap();

    let _ = tokio::time::timeout(Duration::from_secs(2), req_task)
        .await
        .unwrap()
        .unwrap();

    let row: Value = reqwest::get(format!("{ledger_url}/human-approvals/{approval_id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(row["decision"], "denied");
    assert_eq!(row["reason"], "too risky");
}
