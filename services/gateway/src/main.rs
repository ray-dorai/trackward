use gateway::config::Config;
use gateway::{build_router_with_registry, AppState};

#[tokio::main]
async fn main() {
    let _otel = gateway::otel::init();

    let config = Config::from_env();
    let state = AppState::new(config.clone());
    let app = build_router_with_registry(state).await;

    let listener = tokio::net::TcpListener::bind(&config.listen_addr)
        .await
        .expect("failed to bind");
    tracing::info!(addr = %config.listen_addr, "gateway listening");
    axum::serve(listener, app).await.expect("server error");
}
