//! RED stub — verify_bundle is implemented in the GREEN commit.

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
}

#[derive(Debug)]
pub struct Verified {
    pub signed_by: String,
    pub evidence_count: usize,
    pub key_id: String,
}

pub fn verify_bundle(_json: &str) -> Result<Verified, VerifyError> {
    unimplemented!("verifier RED")
}
