//! Ed25519 signing for export bundles.
//!
//! In production a stable keypair is a hard requirement — the verifier
//! pins the public key, so losing it means every prior bundle becomes
//! unverifiable. `SigningService::load` enforces that: when env is
//! `"production"`, a missing `LEDGER_SIGNING_KEY_HEX` is an error, not
//! a convenience. In any other mode (`"development"`, `"test"`, unset)
//! we generate a fresh keypair so local work isn't gated on provisioning.
//!
//! `key_id` is `sha256(public_key_bytes)` — short, deterministic, safe
//! to log, and what operators pin in downstream verifiers.

use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum SigningError {
    #[error("LEDGER_SIGNING_KEY_HEX is required when LEDGER_ENV=production")]
    KeyRequiredInProduction,
    #[error("LEDGER_SIGNING_KEY_HEX is not valid hex: {0}")]
    NotHex(String),
    #[error("LEDGER_SIGNING_KEY_HEX must decode to 32 bytes, got {0}")]
    WrongLength(usize),
}

#[derive(Clone)]
pub struct SigningService {
    signing_key: SigningKey,
    pub key_id: String,
    pub public_key_hex: String,
}

impl SigningService {
    /// Build a SigningService from an explicit env + optional key hex.
    /// This is the testable entry point; `from_env` wraps it.
    pub fn load(env: &str, key_hex: Option<&str>) -> Result<Self, SigningError> {
        let signing_key = match (env, key_hex) {
            (_, Some(hex_str)) => {
                let bytes = hex::decode(hex_str.trim())
                    .map_err(|e| SigningError::NotHex(e.to_string()))?;
                let arr: [u8; 32] = bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| SigningError::WrongLength(bytes.len()))?;
                SigningKey::from_bytes(&arr)
            }
            ("production", None) => return Err(SigningError::KeyRequiredInProduction),
            (_, None) => SigningKey::generate(&mut OsRng),
        };
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.to_bytes());
        let key_id = hex::encode(Sha256::digest(verifying_key.to_bytes()));
        Ok(Self {
            signing_key,
            key_id,
            public_key_hex,
        })
    }

    pub fn from_env() -> Self {
        let env = std::env::var("LEDGER_ENV").unwrap_or_else(|_| "development".into());
        let key_hex = std::env::var("LEDGER_SIGNING_KEY_HEX").ok();
        Self::load(&env, key_hex.as_deref())
            .unwrap_or_else(|e| panic!("cannot initialize SigningService: {e}"))
    }

    pub fn sign_hex(&self, bytes: &[u8]) -> String {
        hex::encode(self.signing_key.sign(bytes).to_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_without_key_is_an_error() {
        let err = SigningService::load("production", None).err().unwrap();
        assert!(matches!(err, SigningError::KeyRequiredInProduction));
    }

    #[test]
    fn production_with_valid_key_loads() {
        // 32 zero bytes is a valid ed25519 seed.
        let hex = "0000000000000000000000000000000000000000000000000000000000000000";
        let svc = SigningService::load("production", Some(hex)).expect("load");
        let svc2 = SigningService::load("production", Some(hex)).expect("load");
        // Same seed → deterministic public key.
        assert_eq!(svc.public_key_hex, svc2.public_key_hex);
        assert_eq!(svc.key_id, svc2.key_id);
    }

    #[test]
    fn development_without_key_generates() {
        // No panic, just a random keypair.
        SigningService::load("development", None).expect("load");
    }

    #[test]
    fn invalid_hex_is_rejected_even_in_development() {
        let err = SigningService::load("development", Some("not-hex")).err().unwrap();
        assert!(matches!(err, SigningError::NotHex(_)));
    }

    #[test]
    fn wrong_length_key_is_rejected() {
        let err = SigningService::load("production", Some("deadbeef")).err().unwrap();
        assert!(matches!(err, SigningError::WrongLength(4)));
    }
}
