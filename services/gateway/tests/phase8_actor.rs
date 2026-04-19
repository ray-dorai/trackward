//! Phase 8c (gateway): every ledger write the gateway makes on a caller's
//! behalf is stamped with the gateway's service-account identity.
//!
//! Two things need to be true for this to be load-bearing:
//!
//! 1. The gateway's `LedgerClient` sends `X-Trackward-Actor` on every write,
//!    so a strict-mode ledger (`default_actor = None`) still accepts its
//!    traffic.
//! 2. The value sent is the gateway's configured `service_account`, not
//!    some per-request shim — because the gateway is the principal doing
//!    the writing, even when it's acting on behalf of an end user.
//!    (Originating user identity, once real auth lands, goes in the event
//!    body or a separate field, not this header.)

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
        let rows: Vec<Value> =
            reqwest::get(format!("{ledger_url}/runs/{run_id}/tool-invocations"))
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

#[tokio::test]
async fn gateway_writes_succeed_against_strict_ledger() {
    // The strongest proof that the gateway sends the header: point it at
    // a ledger that rejects unlabeled writes. If the gateway forgot to
    // forward `X-Trackward-Actor`, run creation alone would 500 and the
    // tool call would never complete.
    let ledger_url = spawn_ledger_with_default_actor(None).await;
    let (backend_url, _backend) =
        spawn_mock_backend(json!({"stdout": "hello", "exit": 0})).await;

    let mut routes = HashMap::new();
    routes.insert("bash".into(), backend_url);
    let gw = spawn_gateway(ledger_url.clone(), routes, None, vec![]).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/tool/bash", gw.url))
        .json(&json!({"command": "echo hi"}))
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

    // And the resulting rows are stamped with the gateway's identity.
    let run: Value = reqwest::get(format!("{ledger_url}/runs/{run_id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(run["actor_id"], "gateway-test");

    let events = get_events(&ledger_url, &run_id).await;
    assert!(!events.is_empty());
    for e in &events {
        assert_eq!(e["actor_id"], "gateway-test", "event: {e:?}");
    }

    let invs = wait_for_tool_invocations(&ledger_url, &run_id, 1).await;
    assert_eq!(invs[0]["actor_id"], "gateway-test");
}
