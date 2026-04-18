//! End-to-end: a bundle produced by the running ledger verifies against
//! the standalone verifier. If the ledger's manifest shape ever drifts
//! from what the verifier expects — key names, JSON encoding, signing
//! input — this test catches it before the next release.

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

#[tokio::test]
async fn ledger_bundle_verifies_with_standalone_verifier() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    // Case + one linked run.
    let run: Value = client
        .post(format!("{base}/runs"))
        .json(&json!({"agent": "e2e-agent"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let run_id = run["id"].as_str().unwrap();

    let case: Value = client
        .post(format!("{base}/cases"))
        .json(&json!({"title": "e2e-case", "opened_by": "ray"}))
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

    let bundle: Value = client
        .post(format!("{base}/cases/{case_id}/exports"))
        .json(&json!({"signed_by": "ray"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let bundle_json = serde_json::to_string(&bundle).unwrap();
    let verified = verifier::verify_bundle(&bundle_json)
        .expect("standalone verifier must accept ledger-produced bundle");
    assert_eq!(verified.signed_by, "ray");
    assert_eq!(verified.evidence_count, 1);
    assert_eq!(verified.key_id, bundle["key_id"].as_str().unwrap());
}
