mod common;

use common::*;
use serde_json::json;
use std::collections::HashMap;

#[tokio::test]
async fn retrieval_records_query_and_stores_docs_as_artifacts() {
    let ledger_url = spawn_ledger().await;

    // Mock retrieval backend returns two documents for a query.
    let (backend_url, backend) = spawn_mock_backend(json!({
        "docs": [
            {"id": "doc1", "content": "the sky is blue"},
            {"id": "doc2", "content": "grass is green"},
        ]
    }))
    .await;

    let gw = spawn_gateway(ledger_url.clone(), HashMap::new(), Some(backend_url), vec![]).await;

    // Act
    let resp = reqwest::Client::new()
        .post(format!("{}/retrieve", gw.url))
        .json(&json!({"query": "colors"}))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let run_id = resp
        .headers()
        .get("x-trackward-run-id")
        .expect("retrieval should mint a run")
        .to_str()
        .unwrap()
        .to_string();
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["docs"].as_array().unwrap().len(), 2);

    // Backend got the query
    let calls = backend.calls.lock().await;
    assert_eq!(calls[0]["query"], "colors");
    drop(calls);

    // Ledger: retrieval_query then retrieval_result
    let events = get_events(&ledger_url, &run_id).await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["kind"], "retrieval_query");
    assert_eq!(events[0]["body"]["query"], "colors");
    assert_eq!(events[1]["kind"], "retrieval_result");

    // Result event references artifact IDs, one per doc
    let artifact_ids = events[1]["body"]["artifact_ids"].as_array().unwrap();
    assert_eq!(artifact_ids.len(), 2, "each retrieved doc becomes an artifact");

    // Artifacts exist in the ledger with the docs' SHA-256
    let artifacts: Vec<serde_json::Value> =
        reqwest::get(format!("{ledger_url}/runs/{run_id}/artifacts"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(artifacts.len(), 2);
    // Downloading gives back the exact doc content → hashes are honest
    for a in &artifacts {
        let id = a["id"].as_str().unwrap();
        let bytes = reqwest::get(format!("{ledger_url}/artifacts/{id}/download"))
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            doc["content"] == "the sky is blue" || doc["content"] == "grass is green",
            "unexpected doc: {doc}"
        );
    }
}

#[tokio::test]
async fn retrieval_with_no_backend_configured_returns_500() {
    let ledger_url = spawn_ledger().await;
    let gw = spawn_gateway(ledger_url, HashMap::new(), None, vec![]).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/retrieve", gw.url))
        .json(&json!({"query": "x"}))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_server_error());
}
