//! Ledger-side glue for the hash-chain.
//!
//! Every run-scoped write goes through [`compute_chain`]:
//!
//! 1. Take a per-(table, run_id) advisory lock inside the caller's
//!    transaction. This serializes concurrent inserts on the same chain
//!    so two callers can't both read the same `prev_hash` and then both
//!    insert, producing two siblings that disagree on the tail.
//! 2. `SELECT row_hash` for the latest non-legacy row in that chain
//!    (partial index `idx_<table>_chain_tail` makes the query O(log n)).
//!    A NULL result means this is the first row in the chain.
//! 3. Hand the caller the bytes it needs to bind: `prev_hash` (or `NULL`
//!    for a fresh chain) and the freshly computed `row_hash`.
//!
//! The caller supplies the canonical-field list for the row it's about
//! to insert (see [`chain_core::CanonicalField`]); this module knows
//! nothing about any specific table's column layout, which keeps the
//! per-route code close to the INSERT it's running.

use chain_core::{canonical_row_bytes, compute_row_hash, CanonicalField, GENESIS_PREV};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::errors::Error;

/// All-zero byte pattern that marks a legacy (pre-Phase-9a) row. The tail
/// lookup filters these out with `row_hash <> decode('0000...', 'hex')`
/// so a new chain starts cleanly after a migration, regardless of what
/// legacy rows exist in the table.
pub const LEGACY_ROW_HASH: [u8; 32] = [0u8; 32];

/// Identifies which per-run chain a table belongs to. Each value becomes
/// part of the advisory-lock key so two different tables' inserts on the
/// same run don't serialize against each other — only inserts into the
/// SAME (table, run_id) block.
///
/// The numeric codes also participate in the canonical row bytes (via
/// the `table` parameter to `canonical_row_bytes`), so moving a row
/// between tables would change its hash — you can't launder an event row
/// into an artifact row and keep the chain valid.
#[derive(Debug, Clone, Copy)]
pub enum ChainTable {
    Events,
    Artifacts,
    ToolInvocations,
    SideEffects,
    Guardrails,
    HumanApprovals,
    BiasSlices,
}

impl ChainTable {
    /// Postgres table name. Matches the `canonical_row_bytes` table
    /// parameter so a verifier uses the same string.
    pub fn name(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Artifacts => "artifacts",
            Self::ToolInvocations => "tool_invocations",
            Self::SideEffects => "side_effects",
            Self::Guardrails => "guardrails",
            Self::HumanApprovals => "human_approvals",
            Self::BiasSlices => "bias_slices",
        }
    }

    /// Per-table discriminator mixed into the advisory-lock key. Must be
    /// globally unique and stable — changing one of these post-launch is
    /// a non-event for data integrity but does invalidate in-flight
    /// transactions at the boundary, so pick distinct primes once and
    /// don't shuffle.
    fn lock_discriminator(self) -> u64 {
        match self {
            Self::Events => 0xE7E1_0001_0000_0000,
            Self::Artifacts => 0xA471_0002_0000_0000,
            Self::ToolInvocations => 0x7001_0003_0000_0000,
            Self::SideEffects => 0x51DE_0004_0000_0000,
            Self::Guardrails => 0x6A21_0005_0000_0000,
            Self::HumanApprovals => 0xB0A0_0006_0000_0000,
            Self::BiasSlices => 0xB1A5_0007_0000_0000,
        }
    }
}

/// Fold a UUID into a u64 for use as an advisory-lock key.
fn uuid_to_u64(id: Uuid) -> u64 {
    let bytes = id.as_bytes();
    let mut key: u64 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        key ^= (b as u64) << ((i % 8) * 8);
    }
    key
}

/// Build a 64-bit advisory-lock key from `(table, run_id)`. We XOR the
/// UUID fold with the per-table discriminator so the space of
/// `(table, run_id)` → key is effectively disjoint across tables.
fn chain_lock_key(table: ChainTable, run_id: Uuid) -> i64 {
    (uuid_to_u64(run_id) ^ table.lock_discriminator()) as i64
}

/// Outcome of a chain compute: the `prev_hash` to bind (NULL for first
/// row in a chain) and the `row_hash` to bind, already hex-ready.
pub struct ChainLink {
    pub prev_hash: Option<Vec<u8>>,
    pub row_hash: Vec<u8>,
}

/// Lock the chain, look up the tail, compute the new link. Call this
/// *inside* a transaction that will also do the `INSERT`; the advisory
/// lock is held at transaction scope, so committing or rolling back
/// releases it.
///
/// `fields` must be the canonical fields of the row about to be
/// inserted — everything the caller intends to persist *except* the
/// chain bookkeeping itself (`prev_hash` and `row_hash`). This is what
/// gets hashed, so if the row you insert ever disagrees with the fields
/// you pass here, the chain will verify-fail later.
pub async fn compute_chain(
    tx: &mut Transaction<'_, Postgres>,
    table: ChainTable,
    run_id: Uuid,
    fields: &[CanonicalField],
) -> Result<ChainLink, Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(chain_lock_key(table, run_id))
        .execute(&mut **tx)
        .await?;

    // Tail lookup — partial index `idx_<table>_chain_tail` skips legacy
    // zero-hash rows, so a brand-new run returns None cleanly even if
    // its table has unrelated pre-Phase-9a rows sitting around.
    let query = format!(
        "SELECT row_hash FROM {} \
         WHERE run_id = $1 AND row_hash <> $2 \
         ORDER BY created_at DESC, id DESC LIMIT 1",
        table.name()
    );
    let prev: Option<Vec<u8>> = sqlx::query_scalar(&query)
        .bind(run_id)
        .bind(&LEGACY_ROW_HASH[..])
        .fetch_optional(&mut **tx)
        .await?;

    let prev_arr: [u8; 32] = match prev.as_deref() {
        Some(bytes) if bytes.len() == 32 => bytes.try_into().unwrap(),
        Some(_) => {
            // The CHECK constraint should prevent this, but if the
            // column was mutated out-of-band we'd rather halt the write
            // than silently chain off a malformed row.
            return Err(Error::Internal(
                "chain tail row has malformed row_hash".into(),
            ));
        }
        None => GENESIS_PREV,
    };

    let row_bytes = canonical_row_bytes(table.name(), fields);
    let row_hash = compute_row_hash(&prev_arr, &row_bytes);

    Ok(ChainLink {
        prev_hash: prev.clone(),
        row_hash: row_hash.to_vec(),
    })
}
