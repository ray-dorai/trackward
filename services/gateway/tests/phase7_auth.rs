//! Phase 7: bearer-token auth on the gateway plus ledger-token
//! forwarding. Two independent concerns tested here:
//!
//! 1. The gateway's own endpoints require a bearer when configured, and
//!    stay open when not (backward-compat).
//! 2. When the ledger is locked down, the gateway uses a configured
//!    ledger-token to keep working — otherwise every recording call fails.

mod common;

use common::{spawn_ledger, spawn_mock_backend, GatewayHarness};
use gateway::config::{Config as GatewayConfig, RegistryBinding};
use gateway::{build_router_with_registry, AppState};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::collections::HashMap;

async fn spawn_gateway_with_tokens(
    ledger_url: String,
    tool_routes: HashMap<String, String>,
    auth_token: Option<String>,
    ledger_token: Option<String>,
) -> GatewayHarness {
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        ledger_url: ledger_url.clone(),
        tool_routes,
        retrieval_backend: None,
        gated_tools: vec![],
        registry: RegistryBinding::default(),
        service_account: "gateway-test".into(),
        tls: None,
        ledger_client_tls: None,
        auth_token,
        ledger_token,
    };
    let state = AppState::new(config);
    let app = build_router_with_registry(state).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    GatewayHarness {
        url: format!("http://{addr}"),
        ledger_url,
    }
}

async fn spawn_ledger_with_token(token: Option<&str>) -> String {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = ledger::config::Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let blob_store = ledger::s3::BlobStore::new(&config).await;
    let state = ledger::AppState::new(pool, blob_store)
        .with_auth_token(token.map(|s| s.to_string()))
        .with_default_actor(Some("phase7-gateway-test".into()));
    let app = ledger::build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn gateway_health_is_always_open() {
    let ledger_url = spawn_ledger().await;
    let hx = spawn_gateway_with_tokens(
        ledger_url,
        HashMap::new(),
        Some("gw-secret".into()),
        None,
    )
    .await;
    let resp = reqwest::get(format!("{}/health", hx.url)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn gateway_rejects_unauthenticated_tool_call() {
    let ledger_url = spawn_ledger().await;
    let (tool_url, _) = spawn_mock_backend(json!({"ok": true})).await;
    let mut routes = HashMap::new();
    routes.insert("echo".into(), tool_url);
    let hx = spawn_gateway_with_tokens(
        ledger_url,
        routes,
        Some("gw-secret".into()),
        None,
    )
    .await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/echo", hx.url))
        .json(&json!({"input": {"x": 1}}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn gateway_accepts_correct_bearer() {
    let ledger_url = spawn_ledger().await;
    let (tool_url, _) = spawn_mock_backend(json!({"ok": true})).await;
    let mut routes = HashMap::new();
    routes.insert("echo".into(), tool_url);
    let hx = spawn_gateway_with_tokens(
        ledger_url,
        routes,
        Some("gw-secret".into()),
        None,
    )
    .await;

    let resp: Value = reqwest::Client::new()
        .post(format!("{}/tool/echo", hx.url))
        .header("Authorization", "Bearer gw-secret")
        .json(&json!({"input": {"x": 1}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(resp["ok"], true);
}

#[tokio::test]
async fn gateway_forwards_ledger_token_on_record_calls() {
    // Lock down the ledger with a token. Without ledger_token configured
    // on the gateway, tool calls fail because recording to the ledger
    // returns 401. With the matching token, recording works again.
    let ledger_url = spawn_ledger_with_token(Some("ledger-secret")).await;
    let (tool_url, _) = spawn_mock_backend(json!({"ok": true})).await;
    let mut routes = HashMap::new();
    routes.insert("echo".into(), tool_url);

    let hx = spawn_gateway_with_tokens(
        ledger_url,
        routes,
        None,
        Some("ledger-secret".into()),
    )
    .await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/echo", hx.url))
        .json(&json!({"input": {"x": 1}}))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_success(),
        "gateway must authenticate to a locked-down ledger; got {}",
        resp.status()
    );
}
