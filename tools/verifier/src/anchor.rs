//! Offline merkle-anchor verification.
//!
//! An anchor document is the signed manifest the ledger shipped to its
//! WORM sink. To verify:
//!
//! 1. sha256(manifest_json) == manifest_sha256
//! 2. ed25519 signature over manifest_json verifies under the embedded
//!    public key
//! 3. Recomputed merkle root over the caller-supplied `row_hashes`
//!    (from a dossier) equals the manifest's `root_hash`
//! 4. Leaf count matches
//!
//! Step 3 is the one that actually catches tampering: (1) and (2) only
//! prove the ledger signed *something*. Without feeding the real
//! dossier's row_hashes back in, an attacker who sees the public key
//! could in principle substitute a legitimate but unrelated anchor
//! document. Tying verification to a specific set of leaves is what
//! binds a dossier to its anchor.

use chain_core::compute_root;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::VerifyError;

/// Outcome of a successful anchor verification.
#[derive(Debug)]
pub struct VerifiedAnchor {
    pub scope: String,
    pub leaf_count: usize,
    pub root_hex: String,
    pub key_id: String,
}

/// Verify an anchor document (the JSON blob the ledger ships to its
/// WORM sink) against a caller-supplied set of 32-byte row_hashes.
///
/// `row_hashes` must be in the same order the ledger used when it
/// built the tree: `(created_at ASC, id ASC)` across the chained
/// tables. The dossier returns rows in that order, so callers can
/// concatenate dossier sections directly.
pub fn verify_anchor(
    doc_json: &str,
    row_hashes: &[[u8; 32]],
) -> Result<VerifiedAnchor, VerifyError> {
    let doc: Value = serde_json::from_str(doc_json)?;

    let manifest_json = field_str(&doc, "manifest_json")?;
    let manifest_sha256 = field_str(&doc, "manifest_sha256")?;
    let signature_hex = field_str(&doc, "signature")?;
    let public_key_hex = field_str(&doc, "public_key_hex")?;
    let key_id = field_str(&doc, "key_id")?;

    let computed = hex::encode(Sha256::digest(manifest_json.as_bytes()));
    if computed != manifest_sha256 {
        return Err(VerifyError::HashMismatch {
            expected: manifest_sha256.to_string(),
            actual: computed,
        });
    }

    let pk_bytes = hex::decode(public_key_hex).map_err(|e| VerifyError::Hex(e.to_string()))?;
    let pk_arr: [u8; 32] = pk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VerifyError::Hex("public_key_hex must be 32 bytes".into()))?;
    let vk = VerifyingKey::from_bytes(&pk_arr).map_err(|_| VerifyError::BadSignature)?;

    let sig_bytes = hex::decode(signature_hex).map_err(|e| VerifyError::Hex(e.to_string()))?;
    let sig_arr: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VerifyError::Hex("signature must be 64 bytes".into()))?;
    let signature = Signature::from_bytes(&sig_arr);

    vk.verify(manifest_json.as_bytes(), &signature)
        .map_err(|_| VerifyError::BadSignature)?;

    let manifest: Value = serde_json::from_str(manifest_json)?;
    let claimed_root = field_str(&manifest, "root_hash")?;
    let claimed_count = manifest
        .get("leaf_count")
        .and_then(|v| v.as_i64())
        .ok_or(VerifyError::MissingField("leaf_count"))?;
    let scope = field_str(&manifest, "scope")?;

    if (claimed_count as usize) != row_hashes.len() {
        return Err(VerifyError::LeafCountMismatch {
            expected: claimed_count as usize,
            actual: row_hashes.len(),
        });
    }
    if row_hashes.is_empty() {
        // Anchors are only minted when the window had leaves; receiving
        // an anchor + zero leaves means the caller failed to collect
        // the dossier rows, not that the anchor is empty.
        return Err(VerifyError::LeafCountMismatch {
            expected: claimed_count as usize,
            actual: 0,
        });
    }

    let computed_root = hex::encode(compute_root(row_hashes));
    if computed_root != claimed_root {
        return Err(VerifyError::RootMismatch {
            expected: claimed_root.to_string(),
            actual: computed_root,
        });
    }

    Ok(VerifiedAnchor {
        scope: scope.to_string(),
        leaf_count: row_hashes.len(),
        root_hex: computed_root,
        key_id: key_id.to_string(),
    })
}

fn field_str<'a>(v: &'a Value, field: &'static str) -> Result<&'a str, VerifyError> {
    v.get(field)
        .and_then(|x| x.as_str())
        .ok_or(VerifyError::MissingField(field))
}
