//! Phase 9a: per-row hash chain.
//!
//! Each run-scoped append-only table chains its rows through SHA-256:
//! row N carries `prev_hash` (= row N-1's `row_hash`) and
//! `row_hash = SHA-256(domain || prev_hash || canonical_bytes(row))`.
//! These tests are the "can a DBA silently rewrite the ledger?" check.
//!
//! Property we want:
//!
//! * Insert three rows; each row's `prev_hash` is the previous row's
//!   `row_hash`; the first has NULL `prev_hash`.
//! * Recompute every row's hash from `(prev_hash, canonical_bytes)` —
//!   matches what's persisted.
//! * Flip a single byte in any persisted column (the row's `body`, its
//!   `actor_id`, its `occurred_at`, anything the chain encoding covers)
//!   and the recomputed `row_hash` no longer matches the stored one.
//! * Coverage across tables: events, tool_invocations, side_effects,
//!   guardrails, human_approvals, artifacts, bias_slices — so the
//!   Phase 9a contract isn't silently bypassed on any write path.

use chain_core::{canonical_row_bytes, compute_row_hash, CanonicalField, GENESIS_PREV};
use chrono::{DateTime, Utc};
use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use serde_json::{json, Value};
use uuid::Uuid;

async fn spawn_server() -> String {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let blob_store = BlobStore::new(&config).await;
    let state = AppState::new(pool, blob_store).with_default_actor(Some("test".into()));
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

async fn create_run(base: &str) -> Uuid {
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&json!({"agent": "phase9-agent"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    Uuid::parse_str(run["id"].as_str().unwrap()).unwrap()
}

fn parse_row_hash(v: &Value) -> [u8; 32] {
    let s = v.as_str().expect("row_hash present");
    hex::decode(s)
        .expect("row_hash hex")
        .try_into()
        .expect("row_hash 32 bytes")
}

fn parse_ts(v: &Value) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(v.as_str().unwrap())
        .unwrap()
        .with_timezone(&Utc)
}

// ------------------------------ events chain -----------------------------

#[tokio::test]
async fn events_chain_is_linked_across_appends() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    // Append three events in sequence.
    let mut rows = Vec::new();
    for i in 0..3 {
        let event: Value = client
            .post(format!("{base}/runs/{run_id}/events"))
            .json(&json!({"kind": format!("step.{i}"), "body": {"i": i}}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        rows.push(event);
    }

    // First row: prev_hash null.
    assert!(rows[0]["prev_hash"].is_null());
    // Subsequent rows: prev_hash == previous row_hash.
    assert_eq!(rows[1]["prev_hash"], rows[0]["row_hash"]);
    assert_eq!(rows[2]["prev_hash"], rows[1]["row_hash"]);

    // Recompute each hash from scratch — this is what a verifier does.
    let mut prev: [u8; 32] = GENESIS_PREV;
    for row in &rows {
        let fields = vec![
            CanonicalField::uuid("id", Uuid::parse_str(row["id"].as_str().unwrap()).unwrap()),
            CanonicalField::uuid(
                "run_id",
                Uuid::parse_str(row["run_id"].as_str().unwrap()).unwrap(),
            ),
            CanonicalField::i64("seq", row["seq"].as_i64().unwrap()),
            CanonicalField::str("kind", row["kind"].as_str().unwrap()),
            CanonicalField::json("body", row["body"].clone()),
            CanonicalField::timestamp("occurred_at", parse_ts(&row["occurred_at"])),
            CanonicalField::str("actor_id", row["actor_id"].as_str().unwrap()),
        ];
        let bytes = canonical_row_bytes("events", &fields);
        let expected = compute_row_hash(&prev, &bytes);
        let stored = parse_row_hash(&row["row_hash"]);
        assert_eq!(
            expected, stored,
            "row {} chain hash mismatch\nrow={row}",
            row["seq"]
        );
        prev = stored;
    }
}

#[tokio::test]
async fn tampering_with_body_breaks_the_chain() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let event: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .json(&json!({"kind": "tool.call", "body": {"truth": "original"}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let stored_hash = parse_row_hash(&event["row_hash"]);
    let prev: [u8; 32] = event["prev_hash"]
        .as_str()
        .map(|s| hex::decode(s).unwrap().try_into().unwrap())
        .unwrap_or(GENESIS_PREV);

    // Verifier on the original row: hash matches.
    let honest_fields = vec![
        CanonicalField::uuid("id", Uuid::parse_str(event["id"].as_str().unwrap()).unwrap()),
        CanonicalField::uuid("run_id", run_id),
        CanonicalField::i64("seq", event["seq"].as_i64().unwrap()),
        CanonicalField::str("kind", "tool.call"),
        CanonicalField::json("body", json!({"truth": "original"})),
        CanonicalField::timestamp("occurred_at", parse_ts(&event["occurred_at"])),
        CanonicalField::str("actor_id", event["actor_id"].as_str().unwrap()),
    ];
    let honest = compute_row_hash(&prev, &canonical_row_bytes("events", &honest_fields));
    assert_eq!(honest, stored_hash);

    // Verifier on a tampered body: hash diverges. This is the attack we
    // care about — a DBA rewrote `body` and we caught it.
    let mut tampered = honest_fields.clone();
    tampered[4] = CanonicalField::json("body", json!({"truth": "fabricated"}));
    let forged = compute_row_hash(&prev, &canonical_row_bytes("events", &tampered));
    assert_ne!(
        forged, stored_hash,
        "tampering with `body` should change the row hash"
    );
}

#[tokio::test]
async fn tampering_with_actor_breaks_the_chain() {
    // Phase 9 depends on Phase 8c — actor_id is inside canonical bytes,
    // so rewriting "who did this" after the fact breaks verification.
    // If this ever passes, the chain hash is no longer covering actor.
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let event: Value = client
        .post(format!("{base}/runs/{run_id}/events"))
        .header("x-trackward-actor", "alice")
        .json(&json!({"kind": "reason", "body": {}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(event["actor_id"], "alice");

    let stored_hash = parse_row_hash(&event["row_hash"]);
    let prev: [u8; 32] = event["prev_hash"]
        .as_str()
        .map(|s| hex::decode(s).unwrap().try_into().unwrap())
        .unwrap_or(GENESIS_PREV);

    let forged_fields = vec![
        CanonicalField::uuid("id", Uuid::parse_str(event["id"].as_str().unwrap()).unwrap()),
        CanonicalField::uuid("run_id", run_id),
        CanonicalField::i64("seq", event["seq"].as_i64().unwrap()),
        CanonicalField::str("kind", "reason"),
        CanonicalField::json("body", json!({})),
        CanonicalField::timestamp("occurred_at", parse_ts(&event["occurred_at"])),
        CanonicalField::str("actor_id", "mallory"), // tampered
    ];
    let forged = compute_row_hash(&prev, &canonical_row_bytes("events", &forged_fields));
    assert_ne!(
        forged, stored_hash,
        "actor_id must participate in the chain hash"
    );
}

#[tokio::test]
async fn chains_across_runs_are_independent() {
    let base = spawn_server().await;
    let run_a = create_run(&base).await;
    let run_b = create_run(&base).await;
    let client = reqwest::Client::new();

    let ev_a: Value = client
        .post(format!("{base}/runs/{run_a}/events"))
        .json(&json!({"kind": "a", "body": {}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ev_b: Value = client
        .post(format!("{base}/runs/{run_b}/events"))
        .json(&json!({"kind": "b", "body": {}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Both are first-in-chain, so both have NULL prev_hash.
    assert!(ev_a["prev_hash"].is_null());
    assert!(ev_b["prev_hash"].is_null());
    // Rows for two different runs live in two different chains — they
    // do not link to each other.
    assert_ne!(ev_a["row_hash"], ev_b["row_hash"]);
}

// ------------------------ multi-table coverage -----------------------
//
// For each non-events chain we spot-check (a) that the returned row
// carries a row_hash and (b) that the first row in the chain has null
// prev_hash. The deep chain-math is covered by events tests; the point
// here is proving the extractor is wired on every write path, not just
// /events.

#[tokio::test]
async fn tool_invocations_carry_row_hash() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let first: Value = client
        .post(format!("{base}/tool-invocations"))
        .json(&json!({"run_id": run_id, "tool": "echo", "status": "ok"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(format!("{base}/tool-invocations"))
        .json(&json!({"run_id": run_id, "tool": "echo", "status": "ok"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(first["prev_hash"].is_null());
    assert_eq!(second["prev_hash"], first["row_hash"]);
    assert_ne!(first["row_hash"], second["row_hash"]);
}

#[tokio::test]
async fn side_effects_carry_row_hash() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let first: Value = client
        .post(format!("{base}/side-effects"))
        .json(&json!({
            "run_id": run_id,
            "kind": "email",
            "target": "a@example.com",
            "status": "sent",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(format!("{base}/side-effects"))
        .json(&json!({
            "run_id": run_id,
            "kind": "email",
            "target": "b@example.com",
            "status": "sent",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(first["prev_hash"].is_null());
    assert_eq!(second["prev_hash"], first["row_hash"]);
}

#[tokio::test]
async fn guardrails_carry_row_hash() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let first: Value = client
        .post(format!("{base}/guardrails"))
        .json(&json!({
            "run_id": run_id,
            "name": "pii",
            "stage": "pre",
            "outcome": "allow",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(first["prev_hash"].is_null());
    assert!(!first["row_hash"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn human_approvals_carry_row_hash() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let approval_id = Uuid::now_v7();
    let first: Value = client
        .post(format!("{base}/human-approvals"))
        .json(&json!({
            "id": approval_id,
            "run_id": run_id,
            "tool": "wire",
            "decision": "granted",
            "requested_at": "2026-04-19T00:00:00Z",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(first["prev_hash"].is_null());
    assert!(!first["row_hash"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn artifacts_carry_row_hash() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let form = reqwest::multipart::Form::new()
        .text("run_id", run_id.to_string())
        .text("label", "phase9-a")
        .text("media_type", "text/plain")
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"hello".to_vec()).file_name("a.txt"),
        );
    let first: Value = client
        .post(format!("{base}/artifacts"))
        .multipart(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(first["prev_hash"].is_null());

    let form2 = reqwest::multipart::Form::new()
        .text("run_id", run_id.to_string())
        .text("label", "phase9-b")
        .text("media_type", "text/plain")
        .part(
            "file",
            reqwest::multipart::Part::bytes(b"world".to_vec()).file_name("b.txt"),
        );
    let second: Value = client
        .post(format!("{base}/artifacts"))
        .multipart(form2)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(second["prev_hash"], first["row_hash"]);
}

#[tokio::test]
async fn bias_slice_without_run_id_is_legacy_zero_chain() {
    // Eval-time bias slices (no run_id) deliberately use the zero
    // legacy marker rather than participating in a chain. They're
    // immutable by eval_result content-hash, which is a separate axis.
    let base = spawn_server().await;
    let client = reqwest::Client::new();

    // Need an eval_result to link — go through the registry.
    let prompt: Value = client
        .post(format!("{base}/prompt-versions"))
        .json(&json!({
            "workflow": "phase9",
            "version": "v1",
            "git_sha": "abc",
            "content_hash": format!("phase9-{}", Uuid::now_v7()),
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
            "workflow": "phase9",
            "version": "v1",
            "prompt_version_id": prompt["id"],
            "git_sha": "abc",
            "content_hash": format!("phase9-eval-{}", Uuid::now_v7()),
            "passed": true,
            "summary": {},
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let slice: Value = client
        .post(format!("{base}/bias-slices"))
        .json(&json!({
            "eval_result_id": eval["id"],
            "label": "eval-time",
            "score": 0.5,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(slice["prev_hash"].is_null());
    assert_eq!(slice["row_hash"].as_str().unwrap(), "0".repeat(64));
}

#[tokio::test]
async fn bias_slice_with_run_id_chains() {
    let base = spawn_server().await;
    let run_id = create_run(&base).await;
    let client = reqwest::Client::new();

    let first: Value = client
        .post(format!("{base}/bias-slices"))
        .json(&json!({
            "run_id": run_id,
            "label": "region",
            "value": "us",
            "score": 0.75,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: Value = client
        .post(format!("{base}/bias-slices"))
        .json(&json!({
            "run_id": run_id,
            "label": "region",
            "value": "eu",
            "score": 0.8,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(first["prev_hash"].is_null());
    assert_eq!(second["prev_hash"], first["row_hash"]);
    // Neither row has the legacy all-zero hash.
    assert_ne!(first["row_hash"].as_str().unwrap(), "0".repeat(64));
}
