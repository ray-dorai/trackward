//! Ed25519 signing for export bundles.
//!
//! The ledger holds a single keypair for its lifetime. On startup we read
//! `LEDGER_SIGNING_KEY_HEX` (32-byte seed, hex-encoded); if unset we
//! generate a fresh keypair — fine for local dev and tests, but any
//! operator running this in anger must supply a stable key so verifiers
//! can pin it.
//!
//! `key_id` is `sha256(public_key_bytes)` — short, deterministic, and safe
//! to log. The full public key also goes into every bundle so an offline
//! verifier never needs to talk back to the ledger.

use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub struct SigningService {
    signing_key: SigningKey,
    pub key_id: String,
    pub public_key_hex: String,
}

impl SigningService {
    pub fn from_env() -> Self {
        let signing_key = match std::env::var("LEDGER_SIGNING_KEY_HEX") {
            Ok(hex_str) => {
                let bytes = hex::decode(hex_str.trim())
                    .expect("LEDGER_SIGNING_KEY_HEX must be hex");
                let arr: [u8; 32] = bytes
                    .as_slice()
                    .try_into()
                    .expect("LEDGER_SIGNING_KEY_HEX must decode to 32 bytes");
                SigningKey::from_bytes(&arr)
            }
            Err(_) => SigningKey::generate(&mut OsRng),
        };
        let verifying_key = signing_key.verifying_key();
        let public_key_hex = hex::encode(verifying_key.to_bytes());
        let key_id = hex::encode(Sha256::digest(verifying_key.to_bytes()));
        Self {
            signing_key,
            key_id,
            public_key_hex,
        }
    }

    pub fn sign_hex(&self, bytes: &[u8]) -> String {
        hex::encode(self.signing_key.sign(bytes).to_bytes())
    }
}
