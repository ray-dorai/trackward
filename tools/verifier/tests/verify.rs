//! Verifier exercises: a well-formed bundle verifies, and every flavor of
//! tamper is rejected. The verifier never talks to the ledger — the bundle
//! is self-contained (public key embedded). If the ledger's signing key
//! rotates, bundles signed under the old key still verify forever as long
//! as their embedded public key is intact.
//!
//! The fixture is built in-process so we don't need a running ledger or a
//! checked-in blob; if the bundle format shifts this test is the canary.

use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn fixture_bundle() -> (SigningKey, Value) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let vk = signing_key.verifying_key();
    let public_key_hex = hex::encode(vk.to_bytes());
    let key_id = hex::encode(Sha256::digest(vk.to_bytes()));

    let manifest = json!({
        "case_id": "00000000-0000-0000-0000-000000000001",
        "generated_at": "2026-04-18T00:00:00Z",
        "evidence": [
            {"type": "run", "id": "00000000-0000-0000-0000-000000000002"}
        ]
    });
    let manifest_json = serde_json::to_string(&manifest).unwrap();
    let manifest_sha256 = hex::encode(Sha256::digest(manifest_json.as_bytes()));
    let signature = hex::encode(signing_key.sign(manifest_json.as_bytes()).to_bytes());

    let bundle = json!({
        "id": "00000000-0000-0000-0000-000000000003",
        "case_id": "00000000-0000-0000-0000-000000000001",
        "manifest_json": manifest_json,
        "manifest_sha256": manifest_sha256,
        "signature": signature,
        "key_id": key_id,
        "public_key_hex": public_key_hex,
        "signed_by": "ray",
        "signed_at": "2026-04-18T00:00:00Z",
    });
    (signing_key, bundle)
}

#[test]
fn valid_bundle_verifies() {
    let (_sk, bundle) = fixture_bundle();
    let json = serde_json::to_string(&bundle).unwrap();
    let result = verifier::verify_bundle(&json).expect("bundle must verify");
    assert_eq!(result.signed_by, "ray");
    assert_eq!(result.evidence_count, 1);
}

#[test]
fn tampered_manifest_is_rejected_by_hash() {
    let (_sk, mut bundle) = fixture_bundle();
    // Alter the manifest_json but leave the original hash/signature.
    bundle["manifest_json"] =
        Value::String(r#"{"case_id":"deadbeef","evidence":[]}"#.into());
    let json = serde_json::to_string(&bundle).unwrap();
    let err = verifier::verify_bundle(&json).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::HashMismatch { .. }),
        "expected hash mismatch, got {err:?}"
    );
}

#[test]
fn swapped_hash_but_bad_signature_is_rejected() {
    // Attacker swaps manifest_json AND recomputes the hash, but can't
    // produce a valid signature without the private key. Should fail at
    // the signature check.
    let (_sk, mut bundle) = fixture_bundle();
    let tampered = r#"{"case_id":"00000000-0000-0000-0000-000000000099","evidence":[]}"#;
    bundle["manifest_json"] = Value::String(tampered.into());
    bundle["manifest_sha256"] =
        Value::String(hex::encode(Sha256::digest(tampered.as_bytes())));
    let json = serde_json::to_string(&bundle).unwrap();
    let err = verifier::verify_bundle(&json).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::BadSignature),
        "expected bad signature, got {err:?}"
    );
}

#[test]
fn wrong_public_key_is_rejected() {
    let (_sk, mut bundle) = fixture_bundle();
    // Replace the embedded public key with a different one — signature
    // was made by the original key, so verify must fail.
    let other = SigningKey::generate(&mut OsRng);
    bundle["public_key_hex"] =
        Value::String(hex::encode(other.verifying_key().to_bytes()));
    let json = serde_json::to_string(&bundle).unwrap();
    let err = verifier::verify_bundle(&json).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::BadSignature),
        "expected bad signature, got {err:?}"
    );
}
