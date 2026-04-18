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

#[derive(Debug, Deserialize)]
pub struct ToolInvocationResponse {
    pub id: Uuid,
}

impl LedgerClient {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base: base.into(),
        }
    }

    /// Like `new`, but pre-configures the client to send
    /// `Authorization: Bearer <token>` on every request. Used when the
    /// ledger has `LEDGER_AUTH_TOKEN` set.
    pub fn with_token(base: impl Into<String>, token: Option<String>) -> Self {
        let Some(token) = token else {
            return Self::new(base);
        };
        let mut headers = reqwest::header::HeaderMap::new();
        let value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .expect("ledger token contains invalid header bytes");
        headers.insert(reqwest::header::AUTHORIZATION, value);
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .expect("failed to build ledger http client");
        Self {
            http,
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

    /// Record a tool_invocation row. Returns the id so the caller can link
    /// downstream side_effects to it.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_tool_invocation(
        &self,
        run_id: Uuid,
        tool: &str,
        input: &serde_json::Value,
        output: &serde_json::Value,
        status: &str,
        status_code: Option<u16>,
    ) -> Result<Uuid, Error> {
        let resp = self
            .http
            .post(format!("{}/tool-invocations", self.base))
            .json(&serde_json::json!({
                "run_id": run_id,
                "tool": tool,
                "input": input,
                "output": output,
                "status": status,
                "status_code": status_code,
            }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "record_tool_invocation {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        let row: ToolInvocationResponse =
            resp.json().await.map_err(|e| Error::Ledger(e.to_string()))?;
        Ok(row.id)
    }

    pub async fn record_side_effect(
        &self,
        run_id: Uuid,
        tool_invocation_id: Option<Uuid>,
        kind: &str,
        target: &str,
        status: &str,
        confirmation: &serde_json::Value,
    ) -> Result<(), Error> {
        let resp = self
            .http
            .post(format!("{}/side-effects", self.base))
            .json(&serde_json::json!({
                "run_id": run_id,
                "tool_invocation_id": tool_invocation_id,
                "kind": kind,
                "target": target,
                "status": status,
                "confirmation": confirmation,
            }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "record_side_effect {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )));
        }
        Ok(())
    }

    /// Write a completed approval (decision already made). The `id` must be
    /// the same approval_id the gateway minted at request time so event log
    /// and row share a key.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_human_approval(
        &self,
        id: Uuid,
        run_id: Uuid,
        tool: &str,
        decision: &str,
        reason: Option<&str>,
        requested_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), Error> {
        let resp = self
            .http
            .post(format!("{}/human-approvals", self.base))
            .json(&serde_json::json!({
                "id": id,
                "run_id": run_id,
                "tool": tool,
                "decision": decision,
                "reason": reason,
                "requested_at": requested_at,
            }))
            .send()
            .await
            .map_err(|e| Error::Ledger(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(Error::Ledger(format!(
                "record_human_approval {} {}",
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
