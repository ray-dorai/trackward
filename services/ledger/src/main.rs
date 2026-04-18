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

    let state = AppState::new(pool, blob_store);
    tracing::info!(
        key_id = %state.signing.key_id,
        "ledger signing key loaded — pin this in verifiers"
    );

    let app = build_router(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .expect("failed to bind");
    tracing::info!(addr = %config.listen_addr, "ledger listening");
    axum::serve(listener, app).await.expect("server error");
}
