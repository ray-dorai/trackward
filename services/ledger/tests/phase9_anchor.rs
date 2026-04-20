//! Phase 9b: merkle anchors.
//!
//! These tests prove:
//!
//! * `anchor_tick` over an empty window returns `None` — no anchor row,
//!   nothing shipped.
//! * `anchor_tick` over a non-empty window builds a merkle tree over
//!   every chained row, signs the root, persists the row, and ships
//!   the signed manifest to the configured sink.
//! * The uploaded document, fed back through the verifier, matches
//!   against the row_hashes collected from the same scope — so a
//!   dossier + anchor pair verifies offline without touching the
//!   ledger.
//! * A second tick after more rows land covers *only* the new rows
//!   (the `anchored_from = previous anchored_to` resume property).
//! * Tampering — either altering a row_hash or dropping a leaf —
//!   breaks the recomputed root under the verifier.
//!
//! Each test uses `AnchorScope::Run(run_id)` so parallel tests don't
//! anchor each other's rows.

use chain_core::compute_root;
use chrono::Utc;
use ledger::anchoring::{anchor_tick, collect_leaves, verify_anchor_doc, AnchorScope};
use ledger::anchors::{AnchorSink, MemorySink};
use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

struct Harness {
    base: String,
    pool: PgPool,
    signing: ledger::signing::SigningService,
    sink: MemorySink,
}

async fn spawn() -> Harness {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");
    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let blob_store = BlobStore::new(&config).await;
    let sink = MemorySink::new();
    let state = AppState::new(pool.clone(), blob_store)
        .with_default_actor(Some("test".into()))
        .with_anchor_sink(AnchorSink::Memory(sink.clone()));
    let signing = state.signing.clone();
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Harness {
        base: format!("http://{addr}"),
        pool,
        signing,
        sink,
    }
}

async fn create_run(base: &str) -> Uuid {
    let run: Value = reqwest::Client::new()
        .post(format!("{base}/runs"))
        .json(&json!({"agent": "phase9b-agent"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    Uuid::parse_str(run["id"].as_str().unwrap()).unwrap()
}

async fn append_event(base: &str, run_id: Uuid, kind: &str) -> Value {
    reqwest::Client::new()
        .post(format!("{base}/runs/{run_id}/events"))
        .json(&json!({"kind": kind, "body": {"marker": kind}}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn empty_window_produces_no_anchor() {
    let h = spawn().await;
    let run_id = create_run(&h.base).await;
    let sink_any = AnchorSink::Memory(h.sink.clone());

    // No events, no chained rows — tick must be a no-op.
    let before = Utc::now();
    let out = anchor_tick(&h.pool, &h.signing, &sink_any, AnchorScope::Run(run_id), before)
        .await
        .unwrap();
    assert!(out.is_none(), "empty scope should not produce an anchor");
}

#[tokio::test]
async fn tick_anchors_all_chained_rows_in_window() {
    let h = spawn().await;
    let run_id = create_run(&h.base).await;

    // Two events + one tool invocation — three chained rows total.
    let _e1 = append_event(&h.base, run_id, "step.1").await;
    let _e2 = append_event(&h.base, run_id, "step.2").await;
    let _ti: Value = reqwest::Client::new()
        .post(format!("{}/tool-invocations", h.base))
        .json(&json!({"run_id": run_id, "tool": "grep", "status": "ok"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let until = Utc::now() + chrono::Duration::seconds(1);
    let sink_any = AnchorSink::Memory(h.sink.clone());
    let anchor = anchor_tick(&h.pool, &h.signing, &sink_any, AnchorScope::Run(run_id), until)
        .await
        .unwrap()
        .expect("anchor should be minted");

    assert_eq!(anchor.leaf_count, 3);
    assert_eq!(anchor.run_id, Some(run_id));
    assert_eq!(anchor.root_hash.len(), 32);
    assert_eq!(anchor.signature.len(), 64);
    // `memory://` prefix mirrors the scheme the sink returns.
    assert!(
        anchor.anchor_target.starts_with("memory://"),
        "unexpected anchor_target: {}",
        anchor.anchor_target
    );

    // Uploaded doc exists under the expected key and verifies offline
    // against the leaves we can re-read from the ledger.
    let key = anchor.anchor_target.trim_start_matches("memory://");
    let doc_bytes = h.sink.get(key).expect("manifest uploaded");
    let doc_json = String::from_utf8(doc_bytes).unwrap();

    let leaves = collect_leaves(
        &h.pool,
        AnchorScope::Run(run_id),
        chrono::DateTime::<Utc>::UNIX_EPOCH,
        until,
    )
    .await
    .unwrap();
    assert_eq!(leaves.len(), 3);
    verify_anchor_doc(&doc_json, &leaves).unwrap();

    // Root the verifier recomputes must equal the row-stored root.
    let recomputed = compute_root(&leaves);
    assert_eq!(recomputed.to_vec(), anchor.root_hash);
}

#[tokio::test]
async fn second_tick_covers_only_new_rows() {
    // Anchor resume: once anchor N is committed, anchor N+1 picks up
    // at `anchored_from = N.anchored_to`. Rewinding or double-covering
    // would let an auditor claim the ledger anchors things it already
    // anchored — silently ambiguous.
    let h = spawn().await;
    let run_id = create_run(&h.base).await;
    let sink_any = AnchorSink::Memory(h.sink.clone());

    append_event(&h.base, run_id, "first.1").await;
    append_event(&h.base, run_id, "first.2").await;
    // Capture `until1` at "now" — not in the future — so the second
    // window actually starts at a bound that's already in the past by
    // the time the next batch of events land.
    let until1 = Utc::now();
    let a1 = anchor_tick(&h.pool, &h.signing, &sink_any, AnchorScope::Run(run_id), until1)
        .await
        .unwrap()
        .expect("first anchor");
    assert_eq!(a1.leaf_count, 2);

    // Sleep past `until1` so new events land strictly after it —
    // without this, `until1 == now` could equal some new created_at
    // at millisecond resolution and the (until1, until2] window would
    // drop the tie.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    append_event(&h.base, run_id, "second.1").await;
    append_event(&h.base, run_id, "second.2").await;
    append_event(&h.base, run_id, "second.3").await;
    let until2 = Utc::now();
    let a2 = anchor_tick(&h.pool, &h.signing, &sink_any, AnchorScope::Run(run_id), until2)
        .await
        .unwrap()
        .expect("second anchor");
    assert_eq!(a2.leaf_count, 3, "second anchor covers only the new rows");
    assert_eq!(a2.anchored_from, a1.anchored_to);

    // Tick with no new work between the two windows → None.
    let until3 = a2.anchored_to;
    let a3 = anchor_tick(&h.pool, &h.signing, &sink_any, AnchorScope::Run(run_id), until3)
        .await
        .unwrap();
    assert!(a3.is_none(), "no rows between a2.anchored_to and itself");
}

#[tokio::test]
async fn tampering_with_a_leaf_breaks_verification() {
    let h = spawn().await;
    let run_id = create_run(&h.base).await;

    append_event(&h.base, run_id, "alpha").await;
    append_event(&h.base, run_id, "beta").await;
    let until = Utc::now() + chrono::Duration::seconds(1);
    let sink_any = AnchorSink::Memory(h.sink.clone());
    let anchor = anchor_tick(&h.pool, &h.signing, &sink_any, AnchorScope::Run(run_id), until)
        .await
        .unwrap()
        .expect("anchor minted");
    let key = anchor.anchor_target.trim_start_matches("memory://");
    let doc_json = String::from_utf8(h.sink.get(key).unwrap()).unwrap();

    let mut leaves = collect_leaves(
        &h.pool,
        AnchorScope::Run(run_id),
        chrono::DateTime::<Utc>::UNIX_EPOCH,
        until,
    )
    .await
    .unwrap();
    assert_eq!(leaves.len(), 2);

    // Tamper: flip a byte of leaf 0 and reverify — must fail.
    leaves[0][0] ^= 0x01;
    let err = verify_anchor_doc(&doc_json, &leaves).unwrap_err();
    assert!(
        format!("{err}").contains("hash mismatch") || format!("{err}").contains("expected"),
        "expected a mismatch, got {err}"
    );
}

#[tokio::test]
async fn dropping_a_leaf_is_detected_as_count_mismatch() {
    let h = spawn().await;
    let run_id = create_run(&h.base).await;

    append_event(&h.base, run_id, "a").await;
    append_event(&h.base, run_id, "b").await;
    append_event(&h.base, run_id, "c").await;
    let until = Utc::now() + chrono::Duration::seconds(1);
    let sink_any = AnchorSink::Memory(h.sink.clone());
    let anchor = anchor_tick(&h.pool, &h.signing, &sink_any, AnchorScope::Run(run_id), until)
        .await
        .unwrap()
        .expect("anchor minted");
    let key = anchor.anchor_target.trim_start_matches("memory://");
    let doc_json = String::from_utf8(h.sink.get(key).unwrap()).unwrap();

    let mut leaves = collect_leaves(
        &h.pool,
        AnchorScope::Run(run_id),
        chrono::DateTime::<Utc>::UNIX_EPOCH,
        until,
    )
    .await
    .unwrap();
    assert_eq!(leaves.len(), 3);

    // Drop a leaf — verifier must refuse on count, not pretend the
    // shorter sequence is fine.
    leaves.pop();
    let err = verify_anchor_doc(&doc_json, &leaves).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("expected") && msg.contains("3"),
        "expected leaf-count error mentioning 3, got: {msg}"
    );
}

#[tokio::test]
async fn trigger_route_produces_anchor() {
    // POST /anchors is an explicit test/operator hook. It's the only
    // way to drive anchoring in a process where the background loop
    // isn't running (which is the Phase 9b test config).
    let h = spawn().await;
    let run_id = create_run(&h.base).await;
    append_event(&h.base, run_id, "via.route").await;

    let resp: Option<Value> = reqwest::Client::new()
        .post(format!("{}/anchors", h.base))
        .json(&json!({"run_id": run_id}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let anchor = resp.expect("route should return Some(anchor)");
    assert_eq!(anchor["leaf_count"], 1);
    assert_eq!(anchor["run_id"], json!(run_id.to_string()));
}
