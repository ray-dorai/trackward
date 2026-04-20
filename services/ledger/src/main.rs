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

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .expect("failed to bind");
    tracing::info!(addr = %config.listen_addr, "ledger listening");
    axum::serve(listener, app).await.expect("server error");
}
