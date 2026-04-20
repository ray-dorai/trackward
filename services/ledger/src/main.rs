use std::sync::Arc;

use ledger::anchors::{AnchorSink, S3Sink};
use ledger::config::Config;
use ledger::s3::BlobStore;
use ledger::{build_router, AppState};

#[tokio::main]
async fn main() {
    let _otel = ledger::otel::init();

    let config = Config::from_env();
    let pool = ledger::db::connect(&config)
        .await
        .expect("failed to connect to database");
    let blob_store = BlobStore::new(&config).await;

    // If ANCHOR_BUCKET is configured, wire an S3 WORM sink and spawn
    // the global anchor loop. Otherwise the sink is Noop — anchor rows
    // can still be produced on demand (e.g. via POST /anchors) but
    // nothing is shipped off-box.
    let anchor_sink = match &config.anchor {
        Some(cfg) => AnchorSink::S3(S3Sink::new(cfg).await),
        None => AnchorSink::Noop,
    };

    let state = AppState::new(pool.clone(), blob_store).with_anchor_sink(anchor_sink.clone());

    if let Some(cfg) = &config.anchor {
        ledger::anchoring::spawn_global_loop(
            pool,
            state.signing.clone(),
            anchor_sink,
            cfg.interval_secs,
        );
        tracing::info!(
            bucket = %cfg.bucket,
            interval_secs = cfg.interval_secs,
            "global anchor loop spawned"
        );
    }

    let app = build_router(state);

    let addr: std::net::SocketAddr = config
        .listen_addr
        .parse()
        .expect("LISTEN_ADDR must be host:port");

    if let Some(tls) = config.tls.as_ref() {
        let server_config = ledger::tls::load_rustls_config(
            &tls.cert_path,
            &tls.key_path,
            &tls.client_ca_path,
        )
        .expect("failed to load TLS config");
        let rustls_config =
            axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(server_config));
        tracing::info!(addr = %addr, "ledger listening (mTLS)");
        axum_server::bind_rustls(addr, rustls_config)
            .serve(app.into_make_service())
            .await
            .expect("server error");
    } else {
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .expect("failed to bind");
        tracing::info!(addr = %addr, "ledger listening (plaintext)");
        axum::serve(listener, app).await.expect("server error");
    }
}
