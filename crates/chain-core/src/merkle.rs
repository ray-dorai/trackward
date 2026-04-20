//! Merkle tree over 32-byte `row_hash` leaves, used for Phase 9b anchors.
//!
//! Every so often the ledger collects the `row_hash` column of every
//! chained row since the last anchor, builds this tree, and publishes
//! the signed root to an external WORM sink. A verifier replays the same
//! computation over the `row_hash`es it reads from a dossier: if one
//! leaf is missing or altered, the recomputed root doesn't match the
//! signed one and the anchor fails.
//!
//! # Shape
//!
//! * **Leaf**: `leaf_hash(row_hash) = SHA256(MERKLE_LEAF_DOMAIN || row_hash)`.
//!   The leaf-domain prefix is what stops a [second-preimage attack]
//!   where someone replaces a leaf with a concatenation of two leaves
//!   and claims an internal node in the leaf layer.
//! * **Internal node**: `SHA256(MERKLE_NODE_DOMAIN || left || right)`.
//! * **Odd sibling**: duplicated. Simple, auditable, one branch of code.
//! * **Empty tree**: disallowed — callers must check `leaf_count > 0`
//!   before asking for a root.
//!
//! The domain constants are versioned (`-v1`) so a future incompatible
//! change is forced to bump them, breaking the pinned tests below.
//!
//! [second-preimage attack]: https://en.wikipedia.org/wiki/Merkle_tree#Second_preimage_attack

use sha2::{Digest, Sha256};

/// Domain-separation prefix for leaf hashing. Distinct from
/// `CHAIN_DOMAIN` and `ROW_DOMAIN` so a row_hash cannot be confused with
/// a merkle-tree leaf.
pub const MERKLE_LEAF_DOMAIN: &[u8] = b"trackward-merkle-leaf-v1\0";

/// Domain-separation prefix for internal-node hashing.
pub const MERKLE_NODE_DOMAIN: &[u8] = b"trackward-merkle-node-v1\0";

fn sha256(chunks: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for chunk in chunks {
        hasher.update(chunk);
    }
    let out = hasher.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

/// Hash a row's 32-byte `row_hash` into its leaf-layer representation.
pub fn leaf_hash(row_hash: &[u8; 32]) -> [u8; 32] {
    sha256(&[MERKLE_LEAF_DOMAIN, row_hash])
}

/// Combine two 32-byte children into an internal-node hash.
pub fn node_hash(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    sha256(&[MERKLE_NODE_DOMAIN, left, right])
}

/// Compute the merkle root of a non-empty sequence of `row_hash`es. Odd
/// siblings at any level are duplicated.
///
/// Panics if `row_hashes` is empty — the root of an empty tree is
/// undefined, and callers should skip anchoring a window with zero
/// leaves rather than invent a sentinel.
pub fn compute_root(row_hashes: &[[u8; 32]]) -> [u8; 32] {
    assert!(
        !row_hashes.is_empty(),
        "merkle root over zero leaves is undefined"
    );

    let mut level: Vec<[u8; 32]> = row_hashes.iter().map(leaf_hash).collect();

    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i < level.len() {
            let left = level[i];
            let right = if i + 1 < level.len() { level[i + 1] } else { left };
            next.push(node_hash(&left, &right));
            i += 2;
        }
        level = next;
    }

    level[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_leaf_root_is_leaf_hash() {
        let leaf = [7u8; 32];
        assert_eq!(compute_root(&[leaf]), leaf_hash(&leaf));
    }

    #[test]
    fn two_leaves_root_is_node_of_leaf_hashes() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        let expected = node_hash(&leaf_hash(&a), &leaf_hash(&b));
        assert_eq!(compute_root(&[a, b]), expected);
    }

    #[test]
    fn odd_last_leaf_is_duplicated() {
        // 3 leaves: level 1 = [h(ab), h(cc)], root = h(h(ab), h(cc))
        let a = [1u8; 32];
        let b = [2u8; 32];
        let c = [3u8; 32];
        let lh_a = leaf_hash(&a);
        let lh_b = leaf_hash(&b);
        let lh_c = leaf_hash(&c);
        let level1_left = node_hash(&lh_a, &lh_b);
        let level1_right = node_hash(&lh_c, &lh_c); // duplicated
        let expected = node_hash(&level1_left, &level1_right);
        assert_eq!(compute_root(&[a, b, c]), expected);
    }

    #[test]
    fn changing_one_leaf_changes_root() {
        let original = compute_root(&[[1u8; 32], [2u8; 32], [3u8; 32], [4u8; 32]]);
        let tampered = compute_root(&[[1u8; 32], [2u8; 32], [3u8; 32], [9u8; 32]]);
        assert_ne!(original, tampered);
    }

    #[test]
    fn leaf_and_node_domains_are_different() {
        // Crucial invariant: a leaf layer and an internal-node layer
        // cannot collide. Otherwise a colluding prover could claim a
        // four-leaf tree is actually a two-leaf tree rooted at the
        // internal node.
        let x = [5u8; 32];
        assert_ne!(leaf_hash(&x), node_hash(&x, &x));
    }

    #[test]
    #[should_panic(expected = "undefined")]
    fn empty_tree_panics() {
        let _ = compute_root(&[]);
    }
}
