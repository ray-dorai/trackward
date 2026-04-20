//! Pinned byte outputs for the merkle anchor tree.
//!
//! Same rules as `pinned_bytes.rs`: any accidental change to the tree
//! construction or domain constants fails here first. A deliberate
//! format bump lives in the same PR as a `MERKLE_*_DOMAIN` version tick.

use chain_core::{
    compute_root, leaf_hash, node_hash, MERKLE_LEAF_DOMAIN, MERKLE_NODE_DOMAIN,
};

#[test]
fn merkle_domains_are_versioned() {
    assert_eq!(MERKLE_LEAF_DOMAIN, b"trackward-merkle-leaf-v1\0");
    assert_eq!(MERKLE_NODE_DOMAIN, b"trackward-merkle-node-v1\0");
}

#[test]
fn pinned_root_for_four_known_leaves() {
    // Deterministic leaves so the root is a magic constant we can pin.
    let leaves = [[0u8; 32], [1u8; 32], [2u8; 32], [3u8; 32]];
    let root = compute_root(&leaves);

    // Recompute inline so this test also double-checks the formula.
    let l0 = leaf_hash(&leaves[0]);
    let l1 = leaf_hash(&leaves[1]);
    let l2 = leaf_hash(&leaves[2]);
    let l3 = leaf_hash(&leaves[3]);
    let n01 = node_hash(&l0, &l1);
    let n23 = node_hash(&l2, &l3);
    let expected = node_hash(&n01, &n23);
    assert_eq!(root, expected, "merkle tree formula drifted");

    // Pinned hex. If this changes, a leaf/node domain constant or the
    // odd-sibling rule moved — review the PR for a version bump.
    assert_eq!(
        hex::encode(root),
        "193d99f22ded6dee973b00cdd40bae3faa6346072ac1ecc05daffeaea2da8d2f"
    );
}
