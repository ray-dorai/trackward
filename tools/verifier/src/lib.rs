//! Offline verifier for trackward export bundles and merkle anchors.
//!
//! Bundles are self-contained: they carry the public key used to sign
//! the manifest, so the verifier runs without any connection to the
//! ledger that produced them. An operator receiving a bundle trusts
//! the *public key*, not the service. Anchors (Phase 9b) follow the
//! same pattern — anchor documents are self-contained too, plus the
//! caller supplies the row_hashes the anchor is supposed to cover.

pub mod anchor;

pub use anchor::{verify_anchor, VerifiedAnchor};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("invalid hex: {0}")]
    Hex(String),
    #[error("bad signature")]
    BadSignature,
    #[error("missing field: {0}")]
    MissingField(&'static str),
    #[error("anchor leaf count mismatch: claimed {expected}, supplied {actual}")]
    LeafCountMismatch { expected: usize, actual: usize },
    #[error("anchor root mismatch: expected {expected}, recomputed {actual}")]
    RootMismatch { expected: String, actual: String },
}

#[derive(Debug)]
pub struct Verified {
    pub signed_by: String,
    pub evidence_count: usize,
    pub key_id: String,
}

pub fn verify_bundle(json: &str) -> Result<Verified, VerifyError> {
    let bundle: Value = serde_json::from_str(json)?;
    let manifest_json = field_str(&bundle, "manifest_json")?;
    let manifest_sha256 = field_str(&bundle, "manifest_sha256")?;
    let signature_hex = field_str(&bundle, "signature")?;
    let public_key_hex = field_str(&bundle, "public_key_hex")?;
    let key_id = field_str(&bundle, "key_id")?;
    let signed_by = field_str(&bundle, "signed_by")?;

    // Hash check.
    let computed = hex::encode(Sha256::digest(manifest_json.as_bytes()));
    if computed != manifest_sha256 {
        return Err(VerifyError::HashMismatch {
            expected: manifest_sha256.to_string(),
            actual: computed,
        });
    }

    // Signature check.
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

    // Count evidence entries for the summary.
    let manifest: Value = serde_json::from_str(manifest_json)?;
    let evidence_count = manifest
        .get("evidence")
        .and_then(|e| e.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    Ok(Verified {
        signed_by: signed_by.to_string(),
        evidence_count,
        key_id: key_id.to_string(),
    })
}

fn field_str<'a>(v: &'a Value, field: &'static str) -> Result<&'a str, VerifyError> {
    v.get(field)
        .and_then(|x| x.as_str())
        .ok_or(VerifyError::MissingField(field))
}
