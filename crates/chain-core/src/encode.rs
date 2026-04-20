//! Row → canonical bytes encoder. See crate docs for the wire format.

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::canonical_json;

/// Magic prefix for row encoding. Bumping this is a chain-format break —
/// every existing row would need a new hash. Keep the version in lockstep
/// with [`crate::chain::CHAIN_DOMAIN`] if it ever changes.
pub const ROW_DOMAIN: &[u8] = b"trackward-row-v1\0";

const TAG_UUID: u8 = 0x01;
const TAG_I64: u8 = 0x02;
const TAG_STR: u8 = 0x03;
const TAG_JSON: u8 = 0x04;
const TAG_TS: u8 = 0x05;
const TAG_BYTES: u8 = 0x06;
const TAG_NULL: u8 = 0x07;
const TAG_BOOL: u8 = 0x08;

/// A single row field: a human-readable name + a typed value. The encoder
/// sorts these by name before serializing, so the call site is free to
/// list them in the order that reads best for a human.
#[derive(Debug, Clone)]
pub struct CanonicalField {
    pub name: &'static str,
    pub value: CanonicalValue,
}

#[derive(Debug, Clone)]
pub enum CanonicalValue {
    Uuid(Uuid),
    I64(i64),
    Str(String),
    /// Stored as `JSONB` on the Postgres side; must be canonicalized with
    /// [`canonical_json::to_canonical_string`] before hashing so two rows
    /// that differ only in key order of a JSON object produce the same hash.
    Json(Value),
    Timestamp(DateTime<Utc>),
    Bytes(Vec<u8>),
    /// `None`-valued column (nullable in the schema).
    Null,
    Bool(bool),
}

impl CanonicalField {
    pub fn uuid(name: &'static str, v: Uuid) -> Self {
        Self { name, value: CanonicalValue::Uuid(v) }
    }
    pub fn i64(name: &'static str, v: i64) -> Self {
        Self { name, value: CanonicalValue::I64(v) }
    }
    pub fn str(name: &'static str, v: impl Into<String>) -> Self {
        Self { name, value: CanonicalValue::Str(v.into()) }
    }
    pub fn json(name: &'static str, v: Value) -> Self {
        Self { name, value: CanonicalValue::Json(v) }
    }
    pub fn timestamp(name: &'static str, v: DateTime<Utc>) -> Self {
        Self { name, value: CanonicalValue::Timestamp(v) }
    }
    pub fn bytes(name: &'static str, v: Vec<u8>) -> Self {
        Self { name, value: CanonicalValue::Bytes(v) }
    }
    pub fn null(name: &'static str) -> Self {
        Self { name, value: CanonicalValue::Null }
    }
    pub fn bool(name: &'static str, v: bool) -> Self {
        Self { name, value: CanonicalValue::Bool(v) }
    }

    /// Optional UUID — `None` becomes a `Null`-typed field. We keep the
    /// field in the encoding even when absent so the hash encodes
    /// "this column exists and is null" distinctly from "this column is
    /// absent from the schema", which matters for forward-compatibility.
    pub fn opt_uuid(name: &'static str, v: Option<Uuid>) -> Self {
        match v {
            Some(u) => Self::uuid(name, u),
            None => Self::null(name),
        }
    }

    pub fn opt_str(name: &'static str, v: Option<impl Into<String>>) -> Self {
        match v {
            Some(s) => Self::str(name, s),
            None => Self::null(name),
        }
    }

    pub fn opt_i64(name: &'static str, v: Option<i64>) -> Self {
        match v {
            Some(n) => Self::i64(name, n),
            None => Self::null(name),
        }
    }

    pub fn opt_bytes(name: &'static str, v: Option<Vec<u8>>) -> Self {
        match v {
            Some(b) => Self::bytes(name, b),
            None => Self::null(name),
        }
    }

    pub fn opt_timestamp(name: &'static str, v: Option<DateTime<Utc>>) -> Self {
        match v {
            Some(t) => Self::timestamp(name, t),
            None => Self::null(name),
        }
    }
}

/// Render `fields` on `table` as the deterministic byte sequence that
/// feeds the row hash. See the module doc for the exact layout.
///
/// `table` is an ASCII identifier matching the Postgres table name
/// (`"events"`, `"tool_invocations"`, etc.). It's included so rows with
/// the same column shape but different provenance (e.g. two tables both
/// holding a single `id` UUID) never collide.
pub fn canonical_row_bytes(table: &str, fields: &[CanonicalField]) -> Vec<u8> {
    assert!(
        table.is_ascii(),
        "table name must be ASCII; got {table:?}"
    );
    assert!(
        table.len() <= u16::MAX as usize,
        "table name too long"
    );
    assert!(
        fields.len() <= u16::MAX as usize,
        "too many fields"
    );

    let mut sorted: Vec<&CanonicalField> = fields.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(b.name));
    // Belt-and-suspenders: a duplicate field name would let two different
    // semantic rows produce the same bytes if the duplicate values were
    // swapped. Callers shouldn't hit this, but we verify.
    for pair in sorted.windows(2) {
        assert!(
            pair[0].name != pair[1].name,
            "duplicate field name {:?} in row encoding",
            pair[0].name
        );
    }

    let mut out = Vec::with_capacity(ROW_DOMAIN.len() + 64);
    out.extend_from_slice(ROW_DOMAIN);
    out.extend_from_slice(&(table.len() as u16).to_be_bytes());
    out.extend_from_slice(table.as_bytes());
    out.extend_from_slice(&(sorted.len() as u16).to_be_bytes());
    for f in sorted {
        encode_field(&mut out, f);
    }
    out
}

fn encode_field(out: &mut Vec<u8>, f: &CanonicalField) {
    assert!(
        f.name.is_ascii(),
        "field name must be ASCII; got {:?}",
        f.name
    );
    out.extend_from_slice(&(f.name.len() as u16).to_be_bytes());
    out.extend_from_slice(f.name.as_bytes());
    match &f.value {
        CanonicalValue::Uuid(u) => {
            out.push(TAG_UUID);
            out.extend_from_slice(u.as_bytes());
        }
        CanonicalValue::I64(n) => {
            out.push(TAG_I64);
            out.extend_from_slice(&n.to_be_bytes());
        }
        CanonicalValue::Str(s) => {
            out.push(TAG_STR);
            write_u32_prefixed(out, s.as_bytes());
        }
        CanonicalValue::Json(v) => {
            out.push(TAG_JSON);
            let canon = canonical_json::to_canonical_string(v);
            write_u32_prefixed(out, canon.as_bytes());
        }
        CanonicalValue::Timestamp(t) => {
            out.push(TAG_TS);
            let s = t.to_rfc3339_opts(SecondsFormat::Micros, true);
            write_u32_prefixed(out, s.as_bytes());
        }
        CanonicalValue::Bytes(b) => {
            out.push(TAG_BYTES);
            write_u32_prefixed(out, b);
        }
        CanonicalValue::Null => {
            out.push(TAG_NULL);
        }
        CanonicalValue::Bool(b) => {
            out.push(TAG_BOOL);
            out.push(if *b { 1 } else { 0 });
        }
    }
}

fn write_u32_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    assert!(bytes.len() <= u32::MAX as usize, "value too long");
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_order_does_not_matter() {
        let a = canonical_row_bytes(
            "t",
            &[CanonicalField::i64("a", 1), CanonicalField::i64("b", 2)],
        );
        let b = canonical_row_bytes(
            "t",
            &[CanonicalField::i64("b", 2), CanonicalField::i64("a", 1)],
        );
        assert_eq!(a, b);
    }

    #[test]
    fn different_tables_differ() {
        let a = canonical_row_bytes("runs", &[CanonicalField::i64("x", 1)]);
        let b = canonical_row_bytes("events", &[CanonicalField::i64("x", 1)]);
        assert_ne!(a, b);
    }

    #[test]
    #[should_panic(expected = "duplicate field name")]
    fn duplicate_field_panics() {
        canonical_row_bytes(
            "t",
            &[CanonicalField::i64("a", 1), CanonicalField::i64("a", 2)],
        );
    }
}
