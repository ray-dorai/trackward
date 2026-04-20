//! Merkle anchor pipeline — the "periodic job" half of Phase 9b.
//!
//! Every tick, for each configured scope:
//!
//!   1. Look up the last anchor's `anchored_to` for this scope — the
//!      start of our window.
//!   2. Collect every `row_hash` across the seven chained tables with
//!      `created_at IN (anchored_from, until_ts]`, in deterministic
//!      order.
//!   3. If there are zero leaves, do nothing — an empty window isn't
//!      an anchor, just a tick where nothing happened.
//!   4. Build the merkle tree, sign the root, ship the signed manifest
//!      to the `AnchorSink`, and persist an `anchors` row pointing at
//!      the resulting URI.
//!
//! The leaf ordering is fixed: `(created_at ASC, id ASC)` across the
//! union of tables. `created_at` is monotonic per transaction and `id`
//! is a v7 UUID (timestamp-prefixed), so this order is reproducible
//! from read-only dossier data. A verifier replays the same sort when
//! reconstructing the tree.
//!
//! Windows are **right-open on the lower edge** and **right-closed on
//! the upper**: `(anchored_from, anchored_to]`. The very first anchor
//! for a scope uses `anchored_from = TIMESTAMP 'epoch'`. This avoids
//! the fence-post bug where a row landing exactly on a boundary is
//! either double-anchored or missed.

use chain_core::compute_root;
use chrono::{DateTime, TimeZone, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::anchors::AnchorSink;
use crate::errors::Error;
use crate::models::Anchor;
use crate::signing::SigningService;

/// Which rows an anchor covers. `Run(id)` anchors only rows belonging
/// to that run; `Global` anchors every chained row in the window.
/// Tests use `Run(_)` for isolation; production timers typically use
/// `Global`.
#[derive(Debug, Clone, Copy)]
pub enum AnchorScope {
    Global,
    Run(Uuid),
}

impl AnchorScope {
    fn run_filter(&self) -> Option<Uuid> {
        match self {
            Self::Global => None,
            Self::Run(id) => Some(*id),
        }
    }

    fn key_segment(&self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Run(id) => format!("run/{id}"),
        }
    }
}

/// Canonical manifest that gets signed and uploaded to the sink. The
/// verifier reconstructs this exact JSON from a dossier + anchor row
/// to validate. Field order is lexicographic to match
/// `chain_core::canonical_json` output.
#[derive(Debug, Clone)]
pub struct AnchorManifest {
    pub version: &'static str,
    pub scope: String,
    pub anchored_from: DateTime<Utc>,
    pub anchored_to: DateTime<Utc>,
    pub leaf_count: i64,
    pub root_hash: [u8; 32],
    pub anchored_at: DateTime<Utc>,
}

impl AnchorManifest {
    /// Build the canonical JSON string that gets signed. This is
    /// deliberately the same shape the verifier recomputes — change
    /// one side and Phase 9b's signature check fails.
    pub fn canonical_json(&self) -> String {
        // Sorted keys, no whitespace, RFC 3339 with 6-digit fractional
        // seconds — matches the encoding rules in `chain_core`.
        format!(
            concat!(
                "{{",
                r#""anchored_at":"{anchored_at}","#,
                r#""anchored_from":"{anchored_from}","#,
                r#""anchored_to":"{anchored_to}","#,
                r#""leaf_count":{leaf_count},"#,
                r#""root_hash":"{root_hex}","#,
                r#""scope":"{scope}","#,
                r#""version":"{version}""#,
                "}}",
            ),
            anchored_at = rfc3339_micros(self.anchored_at),
            anchored_from = rfc3339_micros(self.anchored_from),
            anchored_to = rfc3339_micros(self.anchored_to),
            leaf_count = self.leaf_count,
            root_hex = hex::encode(self.root_hash),
            scope = self.scope,
            version = self.version,
        )
    }
}

fn rfc3339_micros(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// The seven chained tables. This list is the canonical source for
/// "what counts as ledger write activity for anchoring purposes" and
/// must stay in sync with `crate::chain::ChainTable`.
const CHAINED_TABLES: &[&str] = &[
    "events",
    "artifacts",
    "tool_invocations",
    "side_effects",
    "guardrails",
    "human_approvals",
    "bias_slices",
];

/// Read the end of the last anchor's window for a scope, or `TIMESTAMP
/// 'epoch'` if no anchor exists yet. Returned timestamp is the
/// exclusive lower bound of the next window.
pub async fn anchor_window_start(
    pool: &PgPool,
    scope: AnchorScope,
) -> Result<DateTime<Utc>, Error> {
    let run_filter = scope.run_filter();
    let last: Option<DateTime<Utc>> = sqlx::query_scalar(
        "SELECT anchored_to FROM anchors \
         WHERE ($1::uuid IS NULL AND run_id IS NULL) \
            OR ($1::uuid IS NOT NULL AND run_id = $1) \
         ORDER BY anchored_to DESC LIMIT 1",
    )
    .bind(run_filter)
    .fetch_optional(pool)
    .await?;

    Ok(last.unwrap_or_else(|| Utc.timestamp_opt(0, 0).unwrap()))
}

/// Collect all `row_hash` leaves for a scope in `(from, until]`,
/// ordered `(created_at, id)` across the union of chained tables. The
/// legacy all-zero row_hash is excluded — those rows predate Phase 9a
/// and aren't part of any chain.
pub async fn collect_leaves(
    pool: &PgPool,
    scope: AnchorScope,
    from: DateTime<Utc>,
    until: DateTime<Utc>,
) -> Result<Vec<[u8; 32]>, Error> {
    let run_filter = scope.run_filter();

    let union: String = CHAINED_TABLES
        .iter()
        .map(|t| {
            format!(
                "SELECT row_hash, created_at, id FROM {t} \
                 WHERE created_at > $1 AND created_at <= $2 \
                   AND row_hash <> decode(repeat('00', 32), 'hex') \
                   AND ($3::uuid IS NULL OR run_id = $3)"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let query = format!("{union} ORDER BY 2 ASC, 3 ASC");

    let rows: Vec<(Vec<u8>, DateTime<Utc>, Uuid)> = sqlx::query_as(&query)
        .bind(from)
        .bind(until)
        .bind(run_filter)
        .fetch_all(pool)
        .await?;

    let mut leaves = Vec::with_capacity(rows.len());
    for (row_hash, _ts, _id) in rows {
        let arr: [u8; 32] = row_hash
            .as_slice()
            .try_into()
            .map_err(|_| Error::Internal("anchor leaf has non-32-byte row_hash".into()))?;
        leaves.push(arr);
    }
    Ok(leaves)
}

/// End-to-end anchor tick: compute the window, collect leaves, build
/// the tree, sign, ship, persist. Returns:
///
/// * `Ok(Some(anchor))` — an anchor was minted and stored.
/// * `Ok(None)` — the window was empty; nothing to anchor. This is the
///   common case when the timer fires on a quiet ledger.
/// * `Err(_)` — the tick failed (DB, sink, signing). The anchor loop
///   should log and retry on the next tick.
pub async fn anchor_tick(
    pool: &PgPool,
    signing: &SigningService,
    sink: &AnchorSink,
    scope: AnchorScope,
    until: DateTime<Utc>,
) -> Result<Option<Anchor>, Error> {
    let from = anchor_window_start(pool, scope).await?;
    if until <= from {
        return Ok(None);
    }

    let leaves = collect_leaves(pool, scope, from, until).await?;
    if leaves.is_empty() {
        return Ok(None);
    }

    let root = compute_root(&leaves);
    let scope_str = match scope {
        AnchorScope::Global => "global".to_string(),
        AnchorScope::Run(id) => format!("run:{id}"),
    };

    let manifest = AnchorManifest {
        version: "trackward-anchor-v1",
        scope: scope_str,
        anchored_from: from,
        anchored_to: until,
        leaf_count: leaves.len() as i64,
        root_hash: root,
        anchored_at: Utc::now(),
    };
    let manifest_json = manifest.canonical_json();
    let signature_hex = signing.sign_hex(manifest_json.as_bytes());
    let signature = hex::decode(&signature_hex)
        .map_err(|e| Error::Internal(format!("sign output not hex: {e}")))?;

    // The uploaded document carries everything a verifier needs to
    // check the signature offline: manifest, its sha256, signature,
    // pubkey, key_id. Same shape as the Phase 5 export bundle so a
    // future "verify an anchor" CLI can reuse code.
    let manifest_sha256 = hex::encode(Sha256::digest(manifest_json.as_bytes()));
    let doc = serde_json::json!({
        "manifest_json": manifest_json,
        "manifest_sha256": manifest_sha256,
        "signature": signature_hex,
        "key_id": signing.key_id,
        "public_key_hex": signing.public_key_hex,
    });
    let doc_bytes = serde_json::to_vec(&doc)
        .map_err(|e| Error::Internal(format!("anchor doc serialize: {e}")))?;

    let id = Uuid::now_v7();
    let key = format!("{}/{}.json", scope.key_segment(), id);
    let target = sink.put(&key, doc_bytes).await?;

    let row = sqlx::query_as::<_, Anchor>(
        "INSERT INTO anchors \
            (id, run_id, anchored_from, anchored_to, leaf_count, \
             root_hash, signature, key_id, public_key_hex, anchor_target, anchored_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         RETURNING *",
    )
    .bind(id)
    .bind(scope.run_filter())
    .bind(from)
    .bind(until)
    .bind(manifest.leaf_count)
    .bind(&root[..])
    .bind(&signature)
    .bind(&signing.key_id)
    .bind(&signing.public_key_hex)
    .bind(&target)
    .bind(manifest.anchored_at)
    .fetch_one(pool)
    .await?;

    tracing::info!(
        anchor_id = %row.id,
        scope = ?scope,
        anchored_from = %from,
        anchored_to = %until,
        leaf_count = row.leaf_count,
        target = %target,
        "anchor committed"
    );

    Ok(Some(row))
}

/// Background loop: tick every `interval_secs` and try to anchor the
/// global scope. Errors are logged and swallowed — a transient DB or
/// S3 failure mustn't kill the loop.
pub fn spawn_global_loop(
    pool: PgPool,
    signing: SigningService,
    sink: AnchorSink,
    interval_secs: u64,
) {
    tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(std::time::Duration::from_secs(interval_secs.max(1)));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let now = Utc::now();
            match anchor_tick(&pool, &signing, &sink, AnchorScope::Global, now).await {
                Ok(Some(anchor)) => tracing::info!(
                    anchor_id = %anchor.id,
                    "global anchor tick produced new anchor",
                ),
                Ok(None) => {}
                Err(e) => tracing::warn!(error = %e, "anchor tick failed"),
            }
        }
    });
}

/// Verify an `AnchorDoc` JSON blob the way an offline auditor would:
/// manifest sha256 matches, signature over `manifest_json` verifies
/// against the embedded public key, and the caller-supplied leaves
/// recompute the embedded `root_hash`. Used by tests and intended as
/// the shape the standalone verifier will adopt in `verifier::anchor`.
pub fn verify_anchor_doc(doc_json: &str, leaves: &[[u8; 32]]) -> Result<(), Error> {
    let doc: serde_json::Value = serde_json::from_str(doc_json)
        .map_err(|e| Error::BadRequest(format!("anchor doc json: {e}")))?;

    let manifest_json = doc
        .get("manifest_json")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::BadRequest("anchor doc missing manifest_json".into()))?;
    let claimed_sha = doc
        .get("manifest_sha256")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::BadRequest("anchor doc missing manifest_sha256".into()))?;
    let signature_hex = doc
        .get("signature")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::BadRequest("anchor doc missing signature".into()))?;
    let public_key_hex = doc
        .get("public_key_hex")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::BadRequest("anchor doc missing public_key_hex".into()))?;

    let actual_sha = hex::encode(Sha256::digest(manifest_json.as_bytes()));
    if actual_sha != claimed_sha {
        return Err(Error::HashMismatch {
            expected: claimed_sha.to_string(),
            actual: actual_sha,
        });
    }

    let pk_bytes = hex::decode(public_key_hex)
        .map_err(|e| Error::BadRequest(format!("public_key_hex: {e}")))?;
    let pk_arr: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::BadRequest("public_key_hex must be 32 bytes".into()))?;
    let vk = VerifyingKey::from_bytes(&pk_arr)
        .map_err(|_| Error::BadRequest("invalid public key".into()))?;

    let sig_bytes =
        hex::decode(signature_hex).map_err(|e| Error::BadRequest(format!("signature: {e}")))?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| Error::BadRequest("signature must be 64 bytes".into()))?;
    let signature = Signature::from_bytes(&sig_arr);

    vk.verify(manifest_json.as_bytes(), &signature)
        .map_err(|_| Error::BadRequest("anchor signature invalid".into()))?;

    let manifest: serde_json::Value = serde_json::from_str(manifest_json)
        .map_err(|e| Error::BadRequest(format!("manifest json: {e}")))?;
    let claimed_root = manifest
        .get("root_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::BadRequest("manifest missing root_hash".into()))?;
    let claimed_count = manifest
        .get("leaf_count")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| Error::BadRequest("manifest missing leaf_count".into()))?;
    if claimed_count as usize != leaves.len() {
        return Err(Error::HashMismatch {
            expected: format!("leaf_count={claimed_count}"),
            actual: format!("leaf_count={}", leaves.len()),
        });
    }
    if leaves.is_empty() {
        return Err(Error::BadRequest("cannot verify anchor with no leaves".into()));
    }

    let computed_root = hex::encode(compute_root(leaves));
    if computed_root != claimed_root {
        return Err(Error::HashMismatch {
            expected: claimed_root.to_string(),
            actual: computed_root,
        });
    }

    Ok(())
}
