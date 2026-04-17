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

#[derive(Debug, Deserialize)]
struct IdRow {
    id: Uuid,
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

    pub async fn register_prompt_version(
        &self,
        workflow: &str,
        version: &str,
        git_sha: &str,
        content_hash: &str,
    ) -> Result<Uuid, Error> {
        let resp = self
            .http
            .post(format!("{}/prompt-versions", self.base))
            .json(&serde_json::json!({
                "workflow": workflow,
                "version": version,
                "git_sha": git_sha,
                "content_hash": content_hash,
            }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "register_prompt_version {}",
                resp.status()
            )));
        }
        let row: IdRow = resp.json().await.map_err(|e| Error::Ledger(e.to_string()))?;
        Ok(row.id)
    }

    pub async fn register_policy_version(
        &self,
        scope: &str,
        version: &str,
        git_sha: &str,
        content_hash: &str,
    ) -> Result<Uuid, Error> {
        let resp = self
            .http
            .post(format!("{}/policy-versions", self.base))
            .json(&serde_json::json!({
                "scope": scope,
                "version": version,
                "git_sha": git_sha,
                "content_hash": content_hash,
            }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "register_policy_version {}",
                resp.status()
            )));
        }
        let row: IdRow = resp.json().await.map_err(|e| Error::Ledger(e.to_string()))?;
        Ok(row.id)
    }

    /// Return the id of the most recent eval_result for (workflow, version),
    /// or None if none have been recorded yet.
    pub async fn latest_eval_result(
        &self,
        workflow: &str,
        version: &str,
    ) -> Result<Option<Uuid>, Error> {
        let resp = self
            .http
            .get(format!("{}/eval-results", self.base))
            .query(&[("workflow", workflow), ("version", version)])
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "latest_eval_result {}",
                resp.status()
            )));
        }
        let rows: Vec<IdRow> = resp.json().await.map_err(|e| Error::Ledger(e.to_string()))?;
        Ok(rows.into_iter().next().map(|r| r.id))
    }

    /// Bind a run to the given versions. Any `None` field is recorded as NULL
    /// on the ledger side — partial bindings are allowed.
    pub async fn bind_run(
        &self,
        run_id: Uuid,
        prompt_version_id: Option<Uuid>,
        policy_version_id: Option<Uuid>,
        eval_result_id: Option<Uuid>,
    ) -> Result<(), Error> {
        let resp = self
            .http
            .post(format!("{}/runs/{}/bindings", self.base, run_id))
            .json(&serde_json::json!({
                "prompt_version_id": prompt_version_id,
                "policy_version_id": policy_version_id,
                "eval_result_id": eval_result_id,
            }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "bind_run {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(())
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
