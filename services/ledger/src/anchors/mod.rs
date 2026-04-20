//! Where anchor manifests get shipped *outside* the ledger's own
//! database — the "external durable location" half of the trust story.
//!
//! Phase 9b requires one anchor destination. We model it as an enum
//! rather than a `dyn` trait so the dispatch is static, there's no
//! extra `async_trait` dependency, and adding a second backend later
//! is a new enum variant touching this one file.
//!
//! Implementations:
//!
//! * [`S3Sink`] — production: S3 object-lock bucket (WORM). Writes a
//!   compliance-mode object with a retention header so the bucket owner
//!   itself cannot delete it inside the retention window.
//! * [`MemorySink`] — tests: records every put into an in-process map so
//!   test assertions can compare what the loop actually shipped against
//!   what the DB row claims it shipped.
//! * [`NoopSink`] — dev loopback when the operator hasn't wired an
//!   anchor bucket yet. Writes nowhere, returns a synthetic URI. The
//!   anchor row still persists so the chain isn't silently disabled;
//!   any auditor looking at an anchor pointing at `noop://` knows the
//!   deploy is misconfigured.

pub mod memory;
pub mod s3;

pub use memory::MemorySink;
pub use s3::S3Sink;

use crate::errors::Error;

/// Destination for signed anchor manifests. The `put` method is
/// idempotent-on-key: uploading the same `key` twice with identical
/// `bytes` is fine; the call sites always write under a fresh UUID
/// anyway, so idempotency is more "safe for retry" than load-bearing.
#[derive(Clone)]
pub enum AnchorSink {
    S3(S3Sink),
    Memory(MemorySink),
    Noop,
}

impl AnchorSink {
    /// Upload `bytes` under `key` and return the URI that an external
    /// reader (verifier, auditor) can use to fetch it back.
    pub async fn put(&self, key: &str, bytes: Vec<u8>) -> Result<String, Error> {
        match self {
            Self::S3(s) => s.put(key, bytes).await,
            Self::Memory(m) => m.put(key, bytes).await,
            Self::Noop => Ok(format!("noop://{key}")),
        }
    }

    /// Short string identifying the backend — good for logs and for
    /// the "anchor_target" prefix when the backend can't supply its
    /// own URI scheme.
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::S3(_) => "s3",
            Self::Memory(_) => "memory",
            Self::Noop => "noop",
        }
    }
}
