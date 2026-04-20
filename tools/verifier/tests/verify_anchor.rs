//! Offline anchor-verification test.
//!
//! Mirrors `tests/verify.rs`: the fixture is built in-process so the
//! verifier is exercised independently of the ledger. If an anchor
//! manifest's wire shape drifts, this test catches it before the
//! verifier ships.

use chain_core::compute_root;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

fn fixture_anchor() -> (SigningKey, Value, Vec<[u8; 32]>) {
    let signing_key = SigningKey::generate(&mut OsRng);
    let vk = signing_key.verifying_key();
    let public_key_hex = hex::encode(vk.to_bytes());
    let key_id = hex::encode(Sha256::digest(vk.to_bytes()));

    // Deterministic leaves so we can recompute the root below.
    let leaves: Vec<[u8; 32]> = (0..5u8).map(|n| [n; 32]).collect();
    let root_hash = compute_root(&leaves);
    let root_hex = hex::encode(root_hash);

    // Matches `AnchorManifest::canonical_json` on the ledger side.
    let manifest_json = format!(
        concat!(
            "{{",
            r#""anchored_at":"2026-04-19T00:00:00.000000Z","#,
            r#""anchored_from":"2026-04-18T00:00:00.000000Z","#,
            r#""anchored_to":"2026-04-19T00:00:00.000000Z","#,
            r#""leaf_count":{leaf_count},"#,
            r#""root_hash":"{root_hex}","#,
            r#""scope":"run:00000000-0000-0000-0000-000000000001","#,
            r#""version":"trackward-anchor-v1""#,
            "}}",
        ),
        leaf_count = leaves.len(),
        root_hex = root_hex,
    );
    let manifest_sha256 = hex::encode(Sha256::digest(manifest_json.as_bytes()));
    let signature = hex::encode(signing_key.sign(manifest_json.as_bytes()).to_bytes());

    let doc = json!({
        "manifest_json": manifest_json,
        "manifest_sha256": manifest_sha256,
        "signature": signature,
        "key_id": key_id,
        "public_key_hex": public_key_hex,
    });
    (signing_key, doc, leaves)
}

#[test]
fn valid_anchor_verifies() {
    let (_sk, doc, leaves) = fixture_anchor();
    let json = serde_json::to_string(&doc).unwrap();
    let v = verifier::verify_anchor(&json, &leaves).expect("anchor must verify");
    assert_eq!(v.leaf_count, 5);
    assert_eq!(v.scope, "run:00000000-0000-0000-0000-000000000001");
}

#[test]
fn tampered_manifest_is_rejected_by_hash() {
    let (_sk, mut doc, leaves) = fixture_anchor();
    // Alter manifest_json but leave hash+sig as-is: fails sha256 first.
    doc["manifest_json"] = Value::String(r#"{"version":"fake"}"#.into());
    let json = serde_json::to_string(&doc).unwrap();
    let err = verifier::verify_anchor(&json, &leaves).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::HashMismatch { .. }),
        "expected HashMismatch, got {err:?}"
    );
}

#[test]
fn swapped_manifest_with_bad_signature_is_rejected() {
    let (_sk, mut doc, leaves) = fixture_anchor();
    let forged = r#"{"version":"trackward-anchor-v1"}"#;
    doc["manifest_json"] = Value::String(forged.into());
    doc["manifest_sha256"] = Value::String(hex::encode(Sha256::digest(forged.as_bytes())));
    let json = serde_json::to_string(&doc).unwrap();
    let err = verifier::verify_anchor(&json, &leaves).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::BadSignature),
        "expected BadSignature, got {err:?}"
    );
}

#[test]
fn leaf_count_mismatch_is_rejected() {
    let (_sk, doc, leaves) = fixture_anchor();
    let json = serde_json::to_string(&doc).unwrap();
    // Supply fewer leaves than the manifest claims.
    let short = &leaves[..leaves.len() - 1];
    let err = verifier::verify_anchor(&json, short).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::LeafCountMismatch { .. }),
        "expected LeafCountMismatch, got {err:?}"
    );
}

#[test]
fn root_mismatch_on_tampered_leaf_is_rejected() {
    let (_sk, doc, mut leaves) = fixture_anchor();
    // Same count, but one leaf has been swapped out. Root recomputes
    // to a different value and the verifier rejects.
    leaves[0] = [0xAA; 32];
    let json = serde_json::to_string(&doc).unwrap();
    let err = verifier::verify_anchor(&json, &leaves).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::RootMismatch { .. }),
        "expected RootMismatch, got {err:?}"
    );
}

#[test]
fn wrong_public_key_is_rejected() {
    let (_sk, mut doc, leaves) = fixture_anchor();
    let other = SigningKey::generate(&mut OsRng);
    doc["public_key_hex"] = Value::String(hex::encode(other.verifying_key().to_bytes()));
    let json = serde_json::to_string(&doc).unwrap();
    let err = verifier::verify_anchor(&json, &leaves).unwrap_err();
    assert!(
        matches!(err, verifier::VerifyError::BadSignature),
        "expected BadSignature, got {err:?}"
    );
}
