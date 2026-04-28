//! mTLS server configuration for the ledger.
//!
//! When `Config::tls_enabled()` returns true, `main.rs` calls
//! [`load_rustls_config`] to build a rustls [`ServerConfig`] that
//!
//! * presents the server cert + key to every incoming TLS handshake, and
//! * requires the client to present a cert chain that verifies against the
//!   configured client CA bundle. Anything else gets rejected at handshake
//!   time, before a request is ever parsed.
//!
//! This is deliberately additive: bearer-token auth still exists on the
//! routes that use it. mTLS is the option customers whose review boards
//! won't accept shared secrets reach for. They plug in their own CA,
//! every gateway/operator cert is minted under that CA, and the ledger
//! trusts no one else.
//!
//! # Shape of the files
//!
//! * `cert_path` — PEM, leaf first, then any intermediates. Single chain.
//! * `key_path` — PEM, either PKCS#8 or RSA or SEC1 (first match wins).
//! * `client_ca_path` — PEM, one or more CA certs concatenated. Any client
//!   whose chain terminates at one of these is accepted.
//!
//! # Failure policy
//!
//! Any parsing or verifier-construction error surfaces as
//! [`TlsError`] and is fatal at startup. A misconfigured TLS stack should
//! take the process down on boot rather than limp along in plaintext.

use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};

/// Install the default crypto provider exactly once per process. Rustls 0.23
/// requires a provider to be selected before any [`ServerConfig`] is built;
/// doing this lazily in the loader keeps `main.rs` honest and test-friendly.
fn ensure_crypto_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        // Ignore the `Result` — if another caller (e.g. reqwest) already
        // installed one, that's fine; we just can't overwrite it.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// Build a rustls [`ServerConfig`] wired for mutual TLS.
pub fn load_rustls_config(
    cert_path: &Path,
    key_path: &Path,
    client_ca_path: &Path,
) -> Result<ServerConfig, TlsError> {
    ensure_crypto_provider();

    let cert_chain = read_cert_chain(cert_path)?;
    if cert_chain.is_empty() {
        return Err(TlsError::EmptyCertChain(cert_path.display().to_string()));
    }
    let key = read_private_key(key_path)?;

    let client_roots = read_client_ca(client_ca_path)?;
    let client_verifier = WebPkiClientVerifier::builder(Arc::new(client_roots))
        .build()
        .map_err(|e| TlsError::Verifier(e.to_string()))?;

    let server_config = ServerConfig::builder()
        .with_client_cert_verifier(client_verifier)
        .with_single_cert(cert_chain, key)
        .map_err(|e| TlsError::ServerConfig(e.to_string()))?;
    Ok(server_config)
}

fn read_cert_chain(path: &Path) -> Result<Vec<CertificateDer<'static>>, TlsError> {
    CertificateDer::pem_file_iter(path)
        .map_err(|e| TlsError::Parse(path.display().to_string(), e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::Parse(path.display().to_string(), e.to_string()))
}

fn read_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    PrivateKeyDer::from_pem_file(path).map_err(|e| match e {
        rustls::pki_types::pem::Error::NoItemsFound => {
            TlsError::NoPrivateKey(path.display().to_string())
        }
        other => TlsError::Parse(path.display().to_string(), other.to_string()),
    })
}

fn read_client_ca(path: &Path) -> Result<RootCertStore, TlsError> {
    let mut store = RootCertStore::empty();
    let mut added = 0usize;
    for item in CertificateDer::pem_file_iter(path)
        .map_err(|e| TlsError::Parse(path.display().to_string(), e.to_string()))?
    {
        let cert = item.map_err(|e| TlsError::Parse(path.display().to_string(), e.to_string()))?;
        store
            .add(cert)
            .map_err(|e| TlsError::Verifier(e.to_string()))?;
        added += 1;
    }
    if added == 0 {
        return Err(TlsError::EmptyCertChain(path.display().to_string()));
    }
    Ok(store)
}

#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    #[error("tls: io error reading {0}: {1}")]
    Io(String, #[source] std::io::Error),
    #[error("tls: parse error in {0}: {1}")]
    Parse(String, String),
    #[error("tls: {0} contains no certificates")]
    EmptyCertChain(String),
    #[error("tls: {0} contains no private key")]
    NoPrivateKey(String),
    #[error("tls: verifier build failed: {0}")]
    Verifier(String),
    #[error("tls: server config build failed: {0}")]
    ServerConfig(String),
}
