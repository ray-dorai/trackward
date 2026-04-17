mod common;

use common::*;
use gateway::config::RegistryBinding;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Helper: poll the ledger until a run has at least `n` events, or time out.
async fn wait_for_events(ledger_url: &str, run_id: &str, n: usize) -> Vec<serde_json::Value> {
    for _ in 0..50 {
        let events = get_events(ledger_url, run_id).await;
        if events.len() >= n {
            return events;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for {n} events on run {run_id}");
}

#[tokio::test]
async fn approval_request_blocks_until_grant() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url.clone(), HashMap::new(), None, vec![]).await;

    // Fire the request in the background — it should block until we decide.
    let gw_url = gw.url.clone();
    let req_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{gw_url}/approval/request"))
            .json(&json!({
                "tool": "deploy-grant-test",
                "reason": "ship v2 to prod",
            }))
            .send()
            .await
            .unwrap()
    });

    // Give the gateway time to register the pending approval and record the event.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        !req_task.is_finished(),
        "request should be blocked until a decision is posted"
    );

    // Find the pending approval by scanning for the approval_requested event on any run.
    // Since we didn't pass a run_id header, the gateway mints one; we discover it via the
    // ledger rather than guessing. For this test we fetch all runs and find ours by tool name.
    let runs: Vec<serde_json::Value> = reqwest::get(format!("{ledger_url}/runs"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Pick the run that has an approval_requested event for "deploy"
    let mut approval_id: Option<String> = None;
    let mut target_run_id: Option<String> = None;
    for run in &runs {
        let rid = run["id"].as_str().unwrap();
        let events = get_events(&ledger_url, rid).await;
        if let Some(ev) = events
            .iter()
            .find(|e| e["kind"] == "approval_requested" && e["body"]["tool"] == "deploy-grant-test")
        {
            approval_id = Some(ev["body"]["approval_id"].as_str().unwrap().to_string());
            target_run_id = Some(rid.to_string());
            break;
        }
    }
    let approval_id = approval_id.expect("approval_requested event should exist with approval_id");
    let run_id = target_run_id.unwrap();

    // Post the grant decision.
    let decide_resp = reqwest::Client::new()
        .post(format!("{}/approval/{approval_id}/decide", gw.url))
        .json(&json!({"decision": "granted", "reason": "looks fine"}))
        .send()
        .await
        .unwrap();
    assert_eq!(decide_resp.status(), 200);

    // Now the original request should complete.
    let resp = tokio::time::timeout(Duration::from_secs(2), req_task)
        .await
        .expect("request should unblock after decide")
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("x-trackward-run-id").unwrap().to_str().unwrap(),
        run_id,
        "response should carry the run_id it was recorded under"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["approval_id"], approval_id);
    assert_eq!(body["decision"], "granted");

    // Ledger: approval_requested then approval_granted
    let events = wait_for_events(&ledger_url, &run_id, 2).await;
    assert_eq!(events.len(), 2, "expected 2 events, got {events:?}");
    assert_eq!(events[0]["kind"], "approval_requested");
    assert_eq!(events[0]["body"]["tool"], "deploy-grant-test");
    assert_eq!(events[0]["body"]["approval_id"], approval_id);
    assert_eq!(events[1]["kind"], "approval_granted");
    assert_eq!(events[1]["body"]["approval_id"], approval_id);
    assert_eq!(events[1]["body"]["reason"], "looks fine");
}

#[tokio::test]
async fn approval_request_records_denial() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url.clone(), HashMap::new(), None, vec![]).await;

    // Caller pre-creates the run so we know its id up front.
    let run: serde_json::Value = reqwest::Client::new()
        .post(format!("{ledger_url}/runs"))
        .json(&json!({"agent": "test"}))
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
            .json(&json!({"tool": "rm -rf /", "reason": "sketchy"}))
            .send()
            .await
            .unwrap()
    });

    // Poll until approval_requested event appears, then grab the approval_id.
    let events = wait_for_events(&ledger_url, &run_id, 1).await;
    let approval_id = events[0]["body"]["approval_id"].as_str().unwrap().to_string();

    // Deny it.
    let decide_resp = reqwest::Client::new()
        .post(format!("{}/approval/{approval_id}/decide", gw.url))
        .json(&json!({"decision": "denied", "reason": "no way"}))
        .send()
        .await
        .unwrap();
    assert_eq!(decide_resp.status(), 200);

    let resp = tokio::time::timeout(Duration::from_secs(2), req_task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["decision"], "denied");

    let events = wait_for_events(&ledger_url, &run_id, 2).await;
    assert_eq!(events[0]["kind"], "approval_requested");
    assert_eq!(events[1]["kind"], "approval_denied");
    assert_eq!(events[1]["body"]["reason"], "no way");
}

#[tokio::test]
async fn decide_on_unknown_approval_returns_404() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url, HashMap::new(), None, vec![]).await;

    let fake_id = "00000000-0000-0000-0000-000000000000";
    let resp = reqwest::Client::new()
        .post(format!("{}/approval/{fake_id}/decide", gw.url))
        .json(&json!({"decision": "granted"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn approval_grant_with_registry_binding_stamps_run() {
    let ledger_url = spawn_ledger().await;

    let registry_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry");
    let binding = RegistryBinding {
        registry_dir: Some(registry_dir),
        prompt_workflow: Some("example-workflow".into()),
        prompt_version: Some("1.0.0".into()),
        policy_scope: Some("global".into()),
        policy_version: Some("1.0.0".into()),
        git_sha: Some("test-sha-approval".into()),
    };

    let gw = spawn_gateway_with_binding(
        ledger_url.clone(),
        HashMap::new(),
        None,
        vec![],
        Some(binding),
    )
    .await;

    let gw_url = gw.url.clone();
    let req_task = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{gw_url}/approval/request"))
            .json(&json!({"tool": "deploy", "reason": "with binding"}))
            .send()
            .await
            .unwrap()
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Find the approval_id from ledger events
    let runs: Vec<serde_json::Value> = reqwest::get(format!("{ledger_url}/runs"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mut approval_id: Option<String> = None;
    let mut run_id: Option<String> = None;
    for run in &runs {
        let rid = run["id"].as_str().unwrap();
        let events = get_events(&ledger_url, rid).await;
        if let Some(ev) = events
            .iter()
            .find(|e| e["kind"] == "approval_requested" && e["body"]["reason"] == "with binding")
        {
            approval_id = Some(ev["body"]["approval_id"].as_str().unwrap().to_string());
            run_id = Some(rid.to_string());
            break;
        }
    }
    let approval_id = approval_id.expect("should find approval_requested event");
    let run_id = run_id.unwrap();

    // Grant it
    reqwest::Client::new()
        .post(format!("{}/approval/{approval_id}/decide", gw.url))
        .json(&json!({"decision": "granted"}))
        .send()
        .await
        .unwrap();

    let resp = tokio::time::timeout(Duration::from_secs(2), req_task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resp.status(), 200);

    // The run should have a version binding
    let binding: serde_json::Value =
        reqwest::get(format!("{ledger_url}/runs/{run_id}/bindings"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(binding["run_id"], run_id);
    assert!(
        binding["prompt_version_id"].is_string(),
        "approval-minted run should carry prompt binding, got {binding:?}"
    );
    assert!(binding["policy_version_id"].is_string());
}
