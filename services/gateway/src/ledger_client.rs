use serde::Deserialize;
use uuid::Uuid;

use crate::errors::Error;

#[derive(Clone)]
pub struct LedgerClient {
    http: reqwest::Client,
    base: String,
}

#[derive(Debug, Deserialize)]
pub struct RunResponse {
    pub id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct EventResponse {
    pub id: Uuid,
    pub seq: i64,
}

#[derive(Debug, Deserialize)]
pub struct ArtifactResponse {
    pub id: Uuid,
    pub sha256: String,
}

impl LedgerClient {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.into(),
        }
    }

    pub async fn create_run(&self, agent: &str, metadata: serde_json::Value) -> Result<Uuid, Error> {
        let resp = self
            .http
            .post(format!("{}/runs", self.base))
            .json(&serde_json::json!({ "agent": agent, "metadata": metadata }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!("create_run {}", resp.status())));
        }
        let run: RunResponse = resp.json().await.map_err(|e| Error::Ledger(e.to_string()))?;
        Ok(run.id)
    }

    pub async fn append_event(
        &self,
        run_id: Uuid,
        kind: &str,
        body: serde_json::Value,
    ) -> Result<EventResponse, Error> {
        let resp = self
            .http
            .post(format!("{}/runs/{}/events", self.base, run_id))
            .json(&serde_json::json!({ "kind": kind, "body": body }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "append_event {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        resp.json().await.map_err(|e| Error::Ledger(e.to_string()))
    }

    pub async fn upload_artifact(
        &self,
        run_id: Uuid,
        label: &str,
        media_type: &str,
        bytes: Vec<u8>,
    ) -> Result<ArtifactResponse, Error> {
        let form = reqwest::multipart::Form::new()
            .text("run_id", run_id.to_string())
            .text("label", label.to_string())
            .text("media_type", media_type.to_string())
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes).file_name(label.to_string()),
            );
        let resp = self
            .http
            .post(format!("{}/artifacts", self.base))
            .multipart(form)
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "upload_artifact {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        resp.json().await.map_err(|e| Error::Ledger(e.to_string()))
    }
}
