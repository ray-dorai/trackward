//! Registry loader.
//!
//! Walks a `registry/` directory, computes a deterministic content hash for a
//! prompt or policy version, and registers it with the ledger so we can bind
//! runs to exact (git_sha, content_hash) pairs.
//!
//! The hash covers every file in the version directory (recursively), in
//! sorted path order, as `relpath\n<sha256 of file bytes>\n`. That means
//! renaming a file, changing bytes, or adding a file all change the hash.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::RegistryBinding;
use crate::errors::Error;
use crate::ledger_client::LedgerClient;

/// Resolved IDs for the versions a gateway is currently bound to. These are
/// computed once at startup from the registry directory and cached in AppState.
#[derive(Clone, Debug, Default)]
pub struct ResolvedBinding {
    pub prompt_version_id: Option<Uuid>,
    pub policy_version_id: Option<Uuid>,
    pub eval_result_id: Option<Uuid>,
}

impl ResolvedBinding {
    pub fn is_empty(&self) -> bool {
        self.prompt_version_id.is_none()
            && self.policy_version_id.is_none()
            && self.eval_result_id.is_none()
    }
}

/// Hash every file under `dir` and combine into a single sha256 hex string.
/// Ignores nothing — if it's a file under the directory, it's in the hash.
pub fn hash_directory(dir: &Path) -> Result<String, Error> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(dir, &mut files)?;
    files.sort();

    let mut outer = Sha256::new();
    for path in &files {
        let rel = path
            .strip_prefix(dir)
            .map_err(|e| Error::Internal(e.to_string()))?;
        let bytes = std::fs::read(path).map_err(|e| Error::Internal(e.to_string()))?;
        let inner = Sha256::digest(&bytes);
        outer.update(rel.to_string_lossy().as_bytes());
        outer.update(b"\n");
        outer.update(hex::encode(inner).as_bytes());
        outer.update(b"\n");
    }
    Ok(hex::encode(outer.finalize()))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), Error> {
    let read = std::fs::read_dir(dir).map_err(|e| Error::Internal(format!("read_dir {dir:?}: {e}")))?;
    for entry in read {
        let entry = entry.map_err(|e| Error::Internal(e.to_string()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Resolve a binding config against a registry directory, registering each
/// version with the ledger and returning the cached IDs. Any missing field in
/// the config is silently skipped — a gateway may have a prompt binding and no
/// policy binding, or vice versa.
pub async fn resolve(
    ledger: &LedgerClient,
    cfg: &RegistryBinding,
) -> Result<ResolvedBinding, Error> {
    let Some(base) = &cfg.registry_dir else {
        return Ok(ResolvedBinding::default());
    };
    let git_sha = cfg.git_sha.clone().unwrap_or_else(|| "unknown".to_string());

    let mut out = ResolvedBinding::default();

    if let (Some(workflow), Some(version)) = (&cfg.prompt_workflow, &cfg.prompt_version) {
        let dir = base.join("prompts").join(workflow).join(version);
        if dir.is_dir() {
            let hash = hash_directory(&dir)?;
            let id = ledger
                .register_prompt_version(workflow, version, &git_sha, &hash)
                .await?;
            out.prompt_version_id = Some(id);
            tracing::info!(workflow, version, %hash, %id, "prompt_version resolved");
        } else {
            tracing::warn!(?dir, "prompt registry dir missing; skipping binding");
        }
    }

    if let (Some(scope), Some(version)) = (&cfg.policy_scope, &cfg.policy_version) {
        let file = base.join("policies").join(scope).join(format!("{version}.yaml"));
        if file.is_file() {
            let bytes = std::fs::read(&file).map_err(|e| Error::Internal(e.to_string()))?;
            let hash = hex::encode(Sha256::digest(&bytes));
            let id = ledger
                .register_policy_version(scope, version, &git_sha, &hash)
                .await?;
            out.policy_version_id = Some(id);
            tracing::info!(scope, version, %hash, %id, "policy_version resolved");
        } else {
            tracing::warn!(?file, "policy registry file missing; skipping binding");
        }
    }

    // Eval results aren't registered from the filesystem at gateway boot —
    // they're written by the prompt-regression CI workflow, then looked up
    // here if a matching row exists. Phase 3 leaves eval_result_id None
    // when no CI result is present yet.
    if let (Some(workflow), Some(version)) = (&cfg.prompt_workflow, &cfg.prompt_version) {
        if let Some(id) = ledger.latest_eval_result(workflow, version).await? {
            out.eval_result_id = Some(id);
            tracing::info!(workflow, version, %id, "eval_result resolved");
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("registry")
    }

    #[test]
    fn hash_directory_is_deterministic() {
        let dir = fixture_root().join("prompts/example-workflow/1.0.0");
        let h1 = hash_directory(&dir).unwrap();
        let h2 = hash_directory(&dir).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64);
    }

    #[test]
    fn different_versions_hash_differently() {
        // Hash the prompt dir vs. a single policy file's parent — different
        // content, different hash. Guards against the "everything hashes to
        // empty-string" bug.
        let prompt = hash_directory(&fixture_root().join("prompts/example-workflow/1.0.0")).unwrap();
        let policy_dir = fixture_root().join("policies/global");
        let policy = hash_directory(&policy_dir).unwrap();
        assert_ne!(prompt, policy);
    }

    #[test]
    fn resolved_binding_is_empty_by_default() {
        let b = ResolvedBinding::default();
        assert!(b.is_empty());
    }
}
