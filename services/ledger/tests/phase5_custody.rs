//! Phase 5: custody events, cases + case_evidence links, signed export bundles.
//!
//! Spec (from build-order): a haruspex can open a case, link evidence, export
//! a signed bundle, and an independent verifier can check it. The bundle is
//! self-contained (public key embedded) so the verifier doesn't need to talk
//! to the ledger.

use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

async fn spawn_server() -> String {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = Config::from_env();
    let pool = ledger::db::connect(&config)
        .await
        .expect("failed to connect — is docker-compose up?");
    let blob_store = BlobStore::new(&config).await;
    let state = AppState { db: pool, blob_store };
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

async fn create_case(base: &str, title: &str, opened_by: &str) -> String {
    let case: Value = reqwest::Client::new()
        .post(format!("{base}/cases"))
        .json(&json!({"title": title, "opened_by": opened_by}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    case["id"].as_str().unwrap().to_string()
}

// ------------------------------ custody_events -----------------------------

#[tokio::test]
async fn create_and_list_custody_events_by_evidence() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "custody-target").await;

    let recorded: Value = client
        .post(format!("{base}/custody-events"))
        .json(&json!({
            "evidence_type": "run",
            "evidence_id": run_id,
            "action": "read",
            "actor": "ray",
            "reason": "investigation",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(recorded["action"], "read");
    assert_eq!(recorded["actor"], "ray");

    // Write a second custody event on the same evidence
    client
        .post(format!("{base}/custody-events"))
        .json(&json!({
            "evidence_type": "run",
            "evidence_id": run_id,
            "action": "export",
            "actor": "ray",
        }))
        .send()
        .await
        .unwrap();

    let rows: Vec<Value> = client
        .get(format!(
            "{base}/custody-events?evidence_type=run&evidence_id={run_id}"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn cannot_update_or_delete_custody_events() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let run_id = create_run(&base, "ce-immutable").await;

    let created: Value = client
        .post(format!("{base}/custody-events"))
        .json(&json!({
            "evidence_type": "run",
            "evidence_id": run_id,
            "action": "read",
            "actor": "ray",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = created["id"].as_str().unwrap().parse().unwrap();

    let update = sqlx::query("UPDATE custody_events SET action = 'delete' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(update.is_err());
    let delete = sqlx::query("DELETE FROM custody_events WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err());
}

// ------------------------------ cases --------------------------------------

#[tokio::test]
async fn create_and_get_case() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    let created: Value = client
        .post(format!("{base}/cases"))
        .json(&json!({
            "title": "Incident 2026-04-18",
            "description": "agent did something weird at 3am",
            "opened_by": "ray",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["title"], "Incident 2026-04-18");
    let id = created["id"].as_str().unwrap();

    let fetched: Value = client
        .get(format!("{base}/cases/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["id"], id);
    assert_eq!(fetched["opened_by"], "ray");
}

#[tokio::test]
async fn cannot_update_or_delete_cases() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let case_id = create_case(&base, "immutable-case", "ray").await;
    let case_uuid: uuid::Uuid = case_id.parse().unwrap();

    let update = sqlx::query("UPDATE cases SET title = 'x' WHERE id = $1")
        .bind(case_uuid)
        .execute(&pool)
        .await;
    assert!(update.is_err());
    let delete = sqlx::query("DELETE FROM cases WHERE id = $1")
        .bind(case_uuid)
        .execute(&pool)
        .await;
    assert!(delete.is_err());
}

// ------------------------------ case_evidence ------------------------------

#[tokio::test]
async fn link_and_list_case_evidence() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let case_id = create_case(&base, "case-link", "ray").await;
    let run_id = create_run(&base, "case-target").await;

    let link: Value = client
        .post(format!("{base}/cases/{case_id}/evidence"))
        .json(&json!({
            "evidence_type": "run",
            "evidence_id": run_id,
            "linked_by": "ray",
            "note": "suspicious run",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(link["case_id"], case_id);
    assert_eq!(link["evidence_type"], "run");
    assert_eq!(link["evidence_id"], run_id);

    let rows: Vec<Value> = client
        .get(format!("{base}/cases/{case_id}/evidence"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["note"], "suspicious run");
}

#[tokio::test]
async fn duplicate_case_evidence_link_rejected() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let case_id = create_case(&base, "dup-case", "ray").await;
    let run_id = create_run(&base, "dup-target").await;

    let first = client
        .post(format!("{base}/cases/{case_id}/evidence"))
        .json(&json!({
            "evidence_type": "run",
            "evidence_id": run_id,
            "linked_by": "ray",
        }))
        .send()
        .await
        .unwrap();
    assert!(first.status().is_success());

    let second = client
        .post(format!("{base}/cases/{case_id}/evidence"))
        .json(&json!({
            "evidence_type": "run",
            "evidence_id": run_id,
            "linked_by": "ray",
        }))
        .send()
        .await
        .unwrap();
    assert!(
        !second.status().is_success(),
        "re-linking same evidence must fail, got {}",
        second.status()
    );
}

#[tokio::test]
async fn cannot_update_or_delete_case_evidence() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let case_id = create_case(&base, "ce-immutable", "ray").await;
    let run_id = create_run(&base, "ce-target").await;
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

    let case_uuid: uuid::Uuid = case_id.parse().unwrap();
    let update = sqlx::query(
        "UPDATE case_evidence SET note = 'x' WHERE case_id = $1",
    )
    .bind(case_uuid)
    .execute(&pool)
    .await;
    assert!(update.is_err());
    let delete = sqlx::query("DELETE FROM case_evidence WHERE case_id = $1")
        .bind(case_uuid)
        .execute(&pool)
        .await;
    assert!(delete.is_err());
}

// ------------------------------ export_bundles -----------------------------

#[tokio::test]
async fn export_case_produces_signed_bundle() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let case_id = create_case(&base, "export-me", "ray").await;
    let run_id = create_run(&base, "export-target").await;

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

    // Bundle shape
    for key in [
        "id",
        "case_id",
        "manifest_json",
        "manifest_sha256",
        "signature",
        "key_id",
        "public_key_hex",
        "signed_by",
        "signed_at",
    ] {
        assert!(
            bundle.get(key).is_some(),
            "bundle missing field {key}; got {bundle:?}"
        );
    }
    assert_eq!(bundle["case_id"], case_id);
    assert_eq!(bundle["signed_by"], "ray");

    // Manifest bytes → sha256 matches manifest_sha256
    let manifest_str = bundle["manifest_json"].as_str().unwrap();
    let computed = hex::encode(Sha256::digest(manifest_str.as_bytes()));
    assert_eq!(
        computed,
        bundle["manifest_sha256"].as_str().unwrap(),
        "manifest_sha256 must be sha256 of manifest_json bytes"
    );

    // Manifest parses as JSON and contains our evidence entry
    let manifest: Value = serde_json::from_str(manifest_str).unwrap();
    let evidence = manifest["evidence"].as_array().unwrap();
    assert!(
        evidence
            .iter()
            .any(|e| e["type"] == "run" && e["id"] == run_id),
        "manifest should include the linked run, got {manifest:?}"
    );

    // Signature verifies with the embedded public key
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let public_key_bytes = hex::decode(bundle["public_key_hex"].as_str().unwrap()).unwrap();
    let pk_arr: [u8; 32] = public_key_bytes.as_slice().try_into().unwrap();
    let pk = VerifyingKey::from_bytes(&pk_arr).unwrap();
    let sig_bytes = hex::decode(bundle["signature"].as_str().unwrap()).unwrap();
    let sig_arr: [u8; 64] = sig_bytes.as_slice().try_into().unwrap();
    let signature = Signature::from_bytes(&sig_arr);
    pk.verify(manifest_str.as_bytes(), &signature)
        .expect("signature must verify against embedded public key over manifest bytes");
}

#[tokio::test]
async fn export_bundle_can_be_fetched_by_id() {
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let case_id = create_case(&base, "fetch-case", "ray").await;

    let bundle: Value = client
        .post(format!("{base}/cases/{case_id}/exports"))
        .json(&json!({"signed_by": "ray"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = bundle["id"].as_str().unwrap();

    let fetched: Value = client
        .get(format!("{base}/export-bundles/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched["id"], id);
    assert_eq!(fetched["signature"], bundle["signature"]);
}

#[tokio::test]
async fn cannot_update_or_delete_export_bundles() {
    let base = spawn_server().await;
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let client = reqwest::Client::new();
    let case_id = create_case(&base, "bundle-immutable", "ray").await;

    let bundle: Value = client
        .post(format!("{base}/cases/{case_id}/exports"))
        .json(&json!({"signed_by": "ray"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id: uuid::Uuid = bundle["id"].as_str().unwrap().parse().unwrap();

    let update =
        sqlx::query("UPDATE export_bundles SET signature = 'x' WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await;
    assert!(update.is_err());
    let delete = sqlx::query("DELETE FROM export_bundles WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err());
}

#[tokio::test]
async fn signing_is_deterministic_for_same_manifest() {
    // Two exports of the same case with no evidence changes between them
    // should produce the same manifest bytes and thus the same signature
    // (ed25519 signing is deterministic). This is a non-obvious invariant
    // worth locking in — it means the signature is a pure function of the
    // manifest, which makes tamper-detection symmetric.
    let base = spawn_server().await;
    let client = reqwest::Client::new();
    let case_id = create_case(&base, "determinism", "ray").await;
    let run_id = create_run(&base, "det-target").await;
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

    let first: Value = client
        .post(format!("{base}/cases/{case_id}/exports"))
        .json(&json!({"signed_by": "ray", "fixed_signed_at": "2026-04-18T00:00:00Z"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(format!("{base}/cases/{case_id}/exports"))
        .json(&json!({"signed_by": "ray", "fixed_signed_at": "2026-04-18T00:00:00Z"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        first["manifest_sha256"], second["manifest_sha256"],
        "same manifest must hash the same"
    );
    assert_eq!(
        first["signature"], second["signature"],
        "ed25519 over same bytes must produce the same signature"
    );
    assert_ne!(
        first["id"], second["id"],
        "each export still gets a new id — the bundle row is new each time"
    );
}
