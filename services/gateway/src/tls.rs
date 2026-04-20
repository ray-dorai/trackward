//! mTLS configuration for the gateway — mirrors `ledger::tls`.
//!
//! Same shape as the ledger: three PEM paths in, one rustls [`ServerConfig`]
//! out. The gateway is the component most likely to terminate customer
//! traffic, so it is the most important place to offer mTLS. It still trusts
//! the ledger separately (see [`crate::ledger_client`]).

use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};

fn ensure_crypto_provider() {
    use std::sync::Once;
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

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
    let mut reader = BufReader::new(
        fs::File::open(path).map_err(|e| TlsError::Io(path.display().to_string(), e))?,
    );
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TlsError::Parse(path.display().to_string(), e.to_string()))
}

fn read_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, TlsError> {
    let mut reader = BufReader::new(
        fs::File::open(path).map_err(|e| TlsError::Io(path.display().to_string(), e))?,
    );
    match rustls_pemfile::private_key(&mut reader)
        .map_err(|e| TlsError::Parse(path.display().to_string(), e.to_string()))?
    {
        Some(k) => Ok(k),
        None => Err(TlsError::NoPrivateKey(path.display().to_string())),
    }
}

fn read_client_ca(path: &Path) -> Result<RootCertStore, TlsError> {
    let mut reader = BufReader::new(
        fs::File::open(path).map_err(|e| TlsError::Io(path.display().to_string(), e))?,
    );
    let mut store = RootCertStore::empty();
    let mut added = 0usize;
    for item in rustls_pemfile::certs(&mut reader) {
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
