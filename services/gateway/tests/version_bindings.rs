//! Phase 3: every run minted by the gateway must carry a version binding
//! referencing the prompt/policy/eval versions registered from `registry/` in
//! git. Default config binds whatever the gateway was started with; callers
//! can override per-request with headers.

mod common;

use common::*;
use gateway::config::RegistryBinding;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

fn registry_fixture_dir() -> PathBuf {
    // workspace-root/registry — tests run from services/gateway
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("registry")
}

fn default_binding() -> RegistryBinding {
    RegistryBinding {
        registry_dir: Some(registry_fixture_dir()),
        prompt_workflow: Some("example-workflow".into()),
        prompt_version: Some("1.0.0".into()),
        policy_scope: Some("global".into()),
        policy_version: Some("1.0.0".into()),
        git_sha: Some("test-sha-0000000000000000000000000000000000".into()),
    }
}

async fn get_binding(ledger_url: &str, run_id: &str) -> Value {
    reqwest::get(format!("{ledger_url}/runs/{run_id}/bindings"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn tool_call_binds_run_to_registered_versions() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, _b) = spawn_mock_backend(json!({"ok": true})).await;

    let mut routes = HashMap::new();
    routes.insert("bash".into(), backend_url);

    let gw = spawn_gateway_with_binding(
        ledger_url.clone(),
        routes,
        None,
        vec![],
        Some(default_binding()),
    )
    .await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw.url))
        .json(&json!({"command": "ls"}))
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

    let binding = get_binding(&ledger_url, &run_id).await;
    assert_eq!(binding["run_id"], run_id);
    assert!(
        binding["prompt_version_id"].is_string(),
        "expected a prompt_version_id, got {binding:?}"
    );
    assert!(binding["policy_version_id"].is_string());

    // And the versions referenced must actually exist in the ledger, with
    // content_hash matching what was on disk.
    let prompt_id = binding["prompt_version_id"].as_str().unwrap();
    let prompt: Value = reqwest::get(format!("{ledger_url}/prompt-versions/{prompt_id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(prompt["workflow"], "example-workflow");
    assert_eq!(prompt["version"], "1.0.0");
    assert!(
        prompt["content_hash"].as_str().unwrap().len() == 64,
        "content_hash must be a 64-char sha256 hex"
    );
    assert_eq!(prompt["git_sha"], "test-sha-0000000000000000000000000000000000");
}

#[tokio::test]
async fn retrieval_call_binds_run_too() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, _b) = spawn_mock_backend(json!({"docs": []})).await;

    let gw = spawn_gateway_with_binding(
        ledger_url.clone(),
        HashMap::new(),
        Some(backend_url),
        vec![],
        Some(default_binding()),
    )
    .await;

    let resp = reqwest::Client::new()
        .post(format!("{}/retrieve", gw.url))
        .json(&json!({"q": "hello"}))
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

    let binding = get_binding(&ledger_url, &run_id).await;
    assert_eq!(binding["run_id"], run_id);
    assert!(binding["prompt_version_id"].is_string());
}

#[tokio::test]
async fn explicit_run_id_is_not_rebound() {
    // If the caller supplied their own run_id, it's their job to bind it.
    // The gateway must not stomp on an existing binding.
    let ledger_url = spawn_ledger().await;
    let (backend_url, _b) = spawn_mock_backend(json!({"ok": true})).await;

    let mut routes = HashMap::new();
    routes.insert("bash".into(), backend_url);

    let gw = spawn_gateway_with_binding(
        ledger_url.clone(),
        routes,
        None,
        vec![],
        Some(default_binding()),
    )
    .await;

    let run: Value = reqwest::Client::new()
        .post(format!("{ledger_url}/runs"))
        .json(&json!({"agent": "caller"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap().to_string();

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw.url))
        .header("x-trackward-run-id", &run_id)
        .json(&json!({"command": "ls"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // No binding was written — caller-owned run, caller-owned binding.
    let fetch = reqwest::get(format!("{ledger_url}/runs/{run_id}/bindings"))
        .await
        .unwrap();
    assert_eq!(fetch.status(), 404);
}

#[tokio::test]
async fn gateway_without_registry_config_skips_binding() {
    let ledger_url = spawn_ledger().await;
    let (backend_url, _b) = spawn_mock_backend(json!({"ok": true})).await;

    let mut routes = HashMap::new();
    routes.insert("bash".into(), backend_url);

    // No registry binding provided — gateway should still work, but not bind.
    let gw = spawn_gateway_with_binding(ledger_url.clone(), routes, None, vec![], None).await;

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
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let fetch = reqwest::get(format!("{ledger_url}/runs/{run_id}/bindings"))
        .await
        .unwrap();
    assert_eq!(fetch.status(), 404);
}

#[tokio::test]
async fn content_hash_is_deterministic_across_runs() {
    // Two gateways, same registry — should register the identical
    // (workflow, version, content_hash), so the same prompt_version row is reused.
    let ledger_url = spawn_ledger().await;
    let (backend_url, _b) = spawn_mock_backend(json!({"ok": true})).await;

    let routes = || {
        let mut m = HashMap::new();
        m.insert("bash".into(), backend_url.clone());
        m
    };

    let gw1 = spawn_gateway_with_binding(
        ledger_url.clone(),
        routes(),
        None,
        vec![],
        Some(default_binding()),
    )
    .await;
    let gw2 = spawn_gateway_with_binding(
        ledger_url.clone(),
        routes(),
        None,
        vec![],
        Some(default_binding()),
    )
    .await;

    let r1 = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw1.url))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    let r2 = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw2.url))
        .json(&json!({}))
        .send()
        .await
        .unwrap();

    let run1 = r1.headers().get("x-trackward-run-id").unwrap().to_str().unwrap().to_string();
    let run2 = r2.headers().get("x-trackward-run-id").unwrap().to_str().unwrap().to_string();
    let b1 = get_binding(&ledger_url, &run1).await;
    let b2 = get_binding(&ledger_url, &run2).await;
    assert_eq!(
        b1["prompt_version_id"], b2["prompt_version_id"],
        "same registry dir must resolve to the same prompt_version row"
    );
    assert_eq!(b1["policy_version_id"], b2["policy_version_id"]);
}
