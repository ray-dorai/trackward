//! Hash chain: `row_hash = SHA256(domain || prev_hash || row_bytes)`.
//!
//! The chain is linear per-run-per-table (the ledger picks the grouping
//! key; this crate just computes the hash). First row in a chain uses
//! [`GENESIS_PREV`] — 32 zero bytes — for `prev_hash`.

use sha2::{Digest, Sha256};

/// Magic prefix for chain hashing. Distinct from [`crate::ROW_DOMAIN`] so
/// someone can't construct a row whose `canonical_bytes` happen to equal
/// a valid chain hash input.
pub const CHAIN_DOMAIN: &[u8] = b"trackward-chain-v1\0";

/// Distinguished all-zero `prev_hash` for the first row in a chain.
pub const GENESIS_PREV: [u8; 32] = [0u8; 32];

/// Compute `row_hash` for a chain link.
///
/// `prev_hash` must be exactly 32 bytes — pass [`GENESIS_PREV`] for the
/// first row in a chain. `row_bytes` comes from
/// [`crate::canonical_row_bytes`].
pub fn compute_row_hash(prev_hash: &[u8; 32], row_bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(CHAIN_DOMAIN);
    hasher.update(prev_hash);
    hasher.update(row_bytes);
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_changes_when_row_bytes_change() {
        let a = compute_row_hash(&GENESIS_PREV, b"row-a");
        let b = compute_row_hash(&GENESIS_PREV, b"row-b");
        assert_ne!(a, b);
    }

    #[test]
    fn hash_changes_when_prev_changes() {
        let a = compute_row_hash(&[0u8; 32], b"row");
        let b = compute_row_hash(&[1u8; 32], b"row");
        assert_ne!(a, b);
    }

    #[test]
    fn deterministic() {
        let a = compute_row_hash(&GENESIS_PREV, b"row");
        let b = compute_row_hash(&GENESIS_PREV, b"row");
        assert_eq!(a, b);
    }
}
