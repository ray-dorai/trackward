//! Phase 7: bearer-token auth on the ledger. When `auth_token` is set
//! every route except `/health` requires `Authorization: Bearer <token>`.
//! When it's None (the dev default) auth is disabled entirely, so every
//! pre-Phase-7 test keeps working without edits.

use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use reqwest::StatusCode;
use serde_json::json;

async fn spawn_with_token(token: Option<&str>) -> String {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let blob_store = BlobStore::new(&config).await;
    let state = AppState::new(pool, blob_store)
        .with_auth_token(token.map(|s| s.to_string()))
        .with_default_actor(Some("phase7-test".into()));
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn health_is_always_unauthenticated() {
    let base = spawn_with_token(Some("s3cret")).await;
    let resp = reqwest::get(format!("{base}/health")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn protected_route_rejects_missing_bearer() {
    let base = spawn_with_token(Some("s3cret")).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&json!({"agent": "no-auth"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_rejects_wrong_bearer() {
    let base = spawn_with_token(Some("s3cret")).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .header("Authorization", "Bearer wrong")
        .json(&json!({"agent": "wrong-auth"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_accepts_correct_bearer() {
    let base = spawn_with_token(Some("s3cret")).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .header("Authorization", "Bearer s3cret")
        .json(&json!({"agent": "authed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn no_token_configured_disables_auth() {
    // Back-compat: existing deployments without LEDGER_AUTH_TOKEN keep
    // accepting unauthenticated requests. This is the same shape every
    // pre-Phase-7 test relies on, so the guarantee has to be explicit.
    let base = spawn_with_token(None).await;
    let resp = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&json!({"agent": "open"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
