//! Phase 10 — mTLS option.
//!
//! Spins up the ledger bound to a rustls server config that requires a
//! client cert chained to a test CA, and asserts three things:
//!
//! 1. A client that presents no cert cannot complete the TLS handshake —
//!    the request fails before it ever reaches a handler.
//! 2. A client that presents a cert signed by the configured CA reaches
//!    `/health` and gets `200 ok`.
//! 3. A client that presents a cert signed by a *different* CA is rejected.
//!
//! Certs are minted in-process with `rcgen` so the test has no filesystem
//! dependencies beyond a temp dir.

use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair,
};

/// A CA plus the keypair that signed it — enough to mint additional leaves
/// later, which the "foreign CA" test needs.
struct Ca {
    cert: Certificate,
    key: KeyPair,
}

impl Ca {
    fn generate() -> Self {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name = DistinguishedName::new();
        params
            .distinguished_name
            .push(DnType::CommonName, "trackward-test-ca");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let cert = params.self_signed(&key).unwrap();
        Self { cert, key }
    }

    fn pem(&self) -> String {
        self.cert.pem()
    }

    /// Mint a leaf signed by this CA. `sans` populate the SubjectAltName.
    fn issue_leaf(&self, common_name: &str, sans: &[String]) -> Leaf {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(sans.to_vec()).unwrap();
        params.distinguished_name = DistinguishedName::new();
        params.distinguished_name.push(DnType::CommonName, common_name);
        let cert = params.signed_by(&key, &self.cert, &self.key).unwrap();
        Leaf {
            cert_pem: cert.pem(),
            key_pem: key.serialize_pem(),
        }
    }
}

struct Leaf {
    cert_pem: String,
    key_pem: String,
}

fn write_pem(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    path
}

struct Harness {
    addr: SocketAddr,
    ca_pem: String,
    client: Leaf,
    _tmp: tempfile::TempDir,
}

async fn spawn_mtls_server() -> Harness {
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");

    // Pre-allocate a port: bind a plain listener, grab its local_addr, drop
    // it, then let axum_server::bind_rustls rebind the same port. A tight
    // TOCTOU race in principle — negligible in a local test.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let tmp = tempfile::tempdir().unwrap();

    let ca = Ca::generate();
    let server_leaf = ca.issue_leaf(
        "trackward-test",
        &[
            "trackward-test".to_string(),
            "localhost".to_string(),
            addr.ip().to_string(),
        ],
    );
    let client_leaf = ca.issue_leaf("gateway-test", &["gateway-test".to_string()]);

    let server_cert_path = write_pem(tmp.path(), "server.crt", &server_leaf.cert_pem);
    let server_key_path = write_pem(tmp.path(), "server.key", &server_leaf.key_pem);
    let client_ca_path = write_pem(tmp.path(), "clients.ca", &ca.pem());

    let server_config =
        ledger::tls::load_rustls_config(&server_cert_path, &server_key_path, &client_ca_path)
            .unwrap();
    let rustls_config =
        axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config));

    let config = Config::from_env();
    let pool = ledger::db::connect(&config).await.unwrap();
    let blob_store = BlobStore::new(&config).await;
    let state = AppState::new(pool, blob_store).with_default_actor(Some("test".into()));
    let app = build_router(state);

    tokio::spawn(async move {
        axum_server::bind_rustls(addr, rustls_config)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });

    // Give axum_server a beat to start listening. The handshake would fail
    // otherwise — not because of TLS, but because nothing is bound yet.
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    Harness {
        addr,
        ca_pem: ca.pem(),
        client: client_leaf,
        _tmp: tmp,
    }
}

fn client_with_identity(h: &Harness) -> reqwest::Client {
    let ca = reqwest::Certificate::from_pem(h.ca_pem.as_bytes()).unwrap();
    let identity = reqwest::Identity::from_pem(
        format!("{}\n{}", h.client.cert_pem, h.client.key_pem).as_bytes(),
    )
    .unwrap();
    reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(ca)
        .identity(identity)
        .resolve("trackward-test", h.addr)
        .build()
        .unwrap()
}

fn client_without_identity(h: &Harness) -> reqwest::Client {
    let ca = reqwest::Certificate::from_pem(h.ca_pem.as_bytes()).unwrap();
    reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(ca)
        .resolve("trackward-test", h.addr)
        .build()
        .unwrap()
}

fn url(h: &Harness, path: &str) -> String {
    format!("https://trackward-test:{}{}", h.addr.port(), path)
}

fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut msgs = vec![err.to_string()];
    let mut cur = err.source();
    while let Some(next) = cur {
        msgs.push(next.to_string());
        cur = next.source();
    }
    msgs.join(" | ")
}

// ---------------- assertions ----------------

#[tokio::test]
async fn health_succeeds_with_valid_client_cert() {
    let h = spawn_mtls_server().await;
    let resp = client_with_identity(&h)
        .get(url(&h, "/health"))
        .send()
        .await
        .expect("valid client cert must complete TLS handshake");
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "ok");
}

#[tokio::test]
async fn handshake_rejected_when_no_client_cert_presented() {
    let h = spawn_mtls_server().await;
    let err = client_without_identity(&h)
        .get(url(&h, "/health"))
        .send()
        .await
        .expect_err("server must reject request with no client cert");
    // Walk the full source chain to get the TLS details — reqwest::Error's
    // Display only shows the top layer, which is usually "error sending
    // request for url".
    let msg = error_chain(&err).to_lowercase();
    assert!(
        msg.contains("certificate")
            || msg.contains("tls")
            || msg.contains("handshake")
            || msg.contains("eof")
            || msg.contains("connection"),
        "unexpected error: {err:#}"
    );
}

#[tokio::test]
async fn handshake_rejected_for_cert_from_foreign_ca() {
    let h = spawn_mtls_server().await;
    // Mint an independent CA + client leaf. The cert is structurally valid
    // but the server's client-CA store doesn't trust it.
    let foreign_ca = Ca::generate();
    let foreign_leaf = foreign_ca.issue_leaf("foreign-client", &["foreign-client".to_string()]);

    let ca = reqwest::Certificate::from_pem(h.ca_pem.as_bytes()).unwrap();
    let identity = reqwest::Identity::from_pem(
        format!("{}\n{}", foreign_leaf.cert_pem, foreign_leaf.key_pem).as_bytes(),
    )
    .unwrap();
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .add_root_certificate(ca)
        .identity(identity)
        .resolve("trackward-test", h.addr)
        .build()
        .unwrap();

    let err = client
        .get(url(&h, "/health"))
        .send()
        .await
        .expect_err("foreign-CA client cert must be rejected");
    // Walk the full source chain to get the TLS details — reqwest::Error's
    // Display only shows the top layer, which is usually "error sending
    // request for url".
    let msg = error_chain(&err).to_lowercase();
    assert!(
        msg.contains("certificate")
            || msg.contains("tls")
            || msg.contains("handshake")
            || msg.contains("unknown")
            || msg.contains("eof")
            || msg.contains("connection"),
        "unexpected error: {err:#}"
    );
}
