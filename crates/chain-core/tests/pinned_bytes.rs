//! Pinned-byte-output tests for the canonical row encoding.
//!
//! These are the load-bearing tests for the hash-chain's stability: if
//! the ledger and the verifier disagree on canonical bytes, *every*
//! bundle ever produced is silently unverifiable. So we pin the exact
//! bytes produced for known inputs and a couple of full row-hashes.
//!
//! A deliberate format change means bumping `ROW_DOMAIN` /
//! `CHAIN_DOMAIN` AND editing these fixtures in the same PR. Any
//! accidental change (a reordered field, a new prefix byte, a JSON
//! escape shifting) fails here first.

use chain_core::{
    canonical_row_bytes, compute_row_hash, CanonicalField, CHAIN_DOMAIN, GENESIS_PREV, ROW_DOMAIN,
};
use chrono::{TimeZone, Utc};
use serde_json::json;
use uuid::Uuid;

fn h(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

#[test]
fn domain_prefixes_are_versioned() {
    // These two values ARE the format version. Changing them is a
    // chain-format break. Pin them so a casual edit doesn't slip
    // through review.
    assert_eq!(ROW_DOMAIN, b"trackward-row-v1\0");
    assert_eq!(CHAIN_DOMAIN, b"trackward-chain-v1\0");
    assert_eq!(GENESIS_PREV, [0u8; 32]);
}

#[test]
fn canonical_bytes_for_minimal_event() {
    // Fixture row: one UUID + one string.
    let id = Uuid::parse_str("01913c4a-8000-7000-8000-000000000001").unwrap();
    let bytes = canonical_row_bytes(
        "events",
        &[
            CanonicalField::uuid("id", id),
            CanonicalField::str("kind", "tool.call"),
        ],
    );

    // Layout: ROW_DOMAIN (17) || u16(6) || "events" || u16(2)
    //      || u16(2) || "id" || 0x01 || uuid_bytes(16)
    //      || u16(4) || "kind" || 0x03 || u32(9) || "tool.call"
    let expected = hex::decode(concat!(
        // b"trackward-row-v1\0"
        "747261636b776172642d726f772d763100",
        // 0x00 0x06 "events"
        "0006", "6576656e7473",
        // 0x00 0x02 number of fields
        "0002",
        // "id" field: u16(2) || "id" || 0x01 || 16 uuid bytes
        "0002", "6964",
        "01",
        "01913c4a800070008000000000000001",
        // "kind" field: u16(4) || "kind" || 0x03 || u32(9) || "tool.call"
        "0004", "6b696e64",
        "03",
        "00000009",
        "746f6f6c2e63616c6c"
    ))
    .unwrap();
    assert_eq!(h(&bytes), h(&expected), "canonical bytes drifted");
}

#[test]
fn canonical_bytes_for_row_with_json_and_timestamp_and_null() {
    let id = Uuid::parse_str("01913c4a-8000-7000-8000-000000000002").unwrap();
    let ts = Utc
        .with_ymd_and_hms(2026, 4, 19, 12, 34, 56)
        .unwrap()
        .with_timezone(&Utc);

    let bytes = canonical_row_bytes(
        "events",
        &[
            CanonicalField::uuid("id", id),
            CanonicalField::timestamp("occurred_at", ts),
            CanonicalField::json("body", json!({"z": 1, "a": [1, 2]})),
            CanonicalField::null("prev_hash"),
        ],
    );

    // We don't pin the full byte string for this one — the json ordering
    // rule is already covered in canonical_json tests and the format
    // round-trip is covered by the minimal-event test above. What we DO
    // pin: the JSON payload is canonicalized (keys sorted) and the
    // timestamp is emitted with 6 fractional digits + Z.
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains(r#"{"a":[1,2],"z":1}"#));
    assert!(s.contains("2026-04-19T12:34:56.000000Z"));
}

#[test]
fn pinned_row_hash_for_minimal_event() {
    let id = Uuid::parse_str("01913c4a-8000-7000-8000-000000000001").unwrap();
    let row_bytes = canonical_row_bytes(
        "events",
        &[
            CanonicalField::uuid("id", id),
            CanonicalField::str("kind", "tool.call"),
        ],
    );
    let hash = compute_row_hash(&GENESIS_PREV, &row_bytes);

    // Recompute from scratch so this test also double-checks the hash
    // formula, not just a magic constant.
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"trackward-chain-v1\0");
    h.update([0u8; 32]);
    h.update(&row_bytes);
    let expected: [u8; 32] = h.finalize().into();
    assert_eq!(hash, expected);

    // And pin the actual bytes so a format drift upstream is caught.
    // If this hash changes, the chain format has been mutated — review
    // the PR for a ROW_DOMAIN or CHAIN_DOMAIN bump.
    assert_eq!(
        hex::encode(hash),
        "2036c312001ef7ab5d97e1122ac19aa42049edd4d8e5ad643dee9b662651f908"
    );
}

#[test]
fn chain_link_includes_previous_hash() {
    let id = Uuid::parse_str("01913c4a-8000-7000-8000-000000000001").unwrap();
    let row_bytes = canonical_row_bytes(
        "events",
        &[CanonicalField::uuid("id", id)],
    );
    let first = compute_row_hash(&GENESIS_PREV, &row_bytes);
    let second = compute_row_hash(&first, &row_bytes); // same row, different prev
    assert_ne!(first, second, "prev_hash must participate in the hash");
}
