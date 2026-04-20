use std::sync::Arc;

use gateway::config::Config;
use gateway::{build_router_with_registry, AppState};

#[tokio::main]
async fn main() {
    let _otel = gateway::otel::init();

    let config = Config::from_env();
    let state = AppState::new(config.clone());
    let app = build_router_with_registry(state).await;

    let addr: std::net::SocketAddr = config
        .listen_addr
        .parse()
        .expect("GATEWAY_LISTEN_ADDR must be host:port");

    if let Some(tls) = config.tls.as_ref() {
        let server_config = gateway::tls::load_rustls_config(
            &tls.cert_path,
            &tls.key_path,
            &tls.client_ca_path,
        )
        .expect("failed to load TLS config");
        let rustls_config = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config));
        tracing::info!(addr = %addr, "gateway listening (mTLS)");
        axum_server::bind_rustls(addr, rustls_config)
            .serve(app.into_make_service())
            .await
            .expect("server error");
    } else {
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .expect("failed to bind");
        tracing::info!(addr = %addr, "gateway listening (plaintext)");
        axum::serve(listener, app).await.expect("server error");
    }
}
