//! Canonical row encoding + SHA-256 hash chain for the trackward ledger.
//!
//! This crate is the **cryptographic spine** that the ledger and the
//! standalone verifier must agree on byte-for-byte. If the encoding drifts
//! between the two, every bundle ever produced becomes unverifiable.
//! Everything exported here is behind a pinned-byte-output test (see
//! `tests/canonical_bytes.rs`) — changing one of those tests is a
//! deliberate chain-format migration, not a refactor.
//!
//! # Wire format
//!
//! For each row, we compute:
//!
//! ```text
//! canonical_bytes(row)
//!     = "trackward-row-v1\0"                // 17 bytes, magic + version
//!    || u16_be(len(table_name))
//!    || table_name_ascii
//!    || u16_be(num_fields)
//!    || sorted_fields...
//!
//! field(name, value)
//!     = u16_be(len(name)) || name_ascii
//!    || u8(type_tag)
//!    || value_encoding
//! ```
//!
//! Fields are sorted lexicographically by name so a reordering in the row
//! struct cannot change the hash. Type tags:
//!
//! | tag  | Rust type        | encoding                                  |
//! |------|------------------|-------------------------------------------|
//! | 0x01 | `Uuid`           | 16 bytes, big-endian                      |
//! | 0x02 | `i64`            | 8 bytes, big-endian                       |
//! | 0x03 | `&str`           | `u32_be(len) || utf8`                     |
//! | 0x04 | `serde_json::Value` | `u32_be(len) || canonical_json_utf8`   |
//! | 0x05 | `DateTime<Utc>`  | `u32_be(len) || rfc3339_6digit_utf8`      |
//! | 0x06 | `&[u8]`          | `u32_be(len) || bytes`                    |
//! | 0x07 | null/None        | zero bytes                                |
//! | 0x08 | `bool`           | 1 byte (0/1)                              |
//!
//! `canonical_json` is RFC 8785–shaped but deliberately narrow: the ledger
//! never stores floats (Postgres `JSONB` numbers come back through
//! `serde_json::Number` as integers or not at all for our schema), so we
//! don't need `ryu`/shortest-roundtrip. Object keys are sorted, whitespace
//! is elided, string escapes are minimal JSON-compliant. If the schema
//! ever gains floats, that field type needs an explicit design pass — not
//! an ad-hoc extension of this encoder.
//!
//! RFC 3339 timestamps are emitted with exactly 6 fractional digits
//! followed by `Z` so Postgres microsecond precision round-trips
//! deterministically. `DateTime<Utc>` in chrono is sub-microsecond
//! internally; we truncate on the ledger side before persist.
//!
//! # Hash chain
//!
//! ```text
//! row_hash = SHA-256(
//!     "trackward-chain-v1\0"          // 19 bytes, magic + version
//!  || prev_hash_or_genesis            // 32 bytes (zeros for first row)
//!  || canonical_bytes(row)
//! )
//! ```
//!
//! First row in a chain hashes over a 32-byte zero buffer — there is no
//! distinguished "genesis" row. An auditor checks a chain by recomputing
//! each row_hash from (prev_hash, row bytes) and comparing to the stored
//! `row_hash`. A row-level tamper changes `canonical_bytes(row)`, which
//! changes `row_hash`, which breaks the link for every subsequent row.

pub mod canonical_json;
pub mod chain;
pub mod encode;

pub use chain::{compute_row_hash, CHAIN_DOMAIN, GENESIS_PREV};
pub use encode::{canonical_row_bytes, CanonicalField, CanonicalValue, ROW_DOMAIN};
