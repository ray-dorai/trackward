pub mod config;
pub mod db;
pub mod errors;
pub mod hash;
pub mod models;
pub mod otel;
pub mod routes;
pub mod s3;

use axum::routing::{get, post};
use axum::Router;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

use crate::s3::BlobStore;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub blob_store: BlobStore,
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/runs", post(routes::runs::create).get(routes::runs::list))
        .route("/runs/{id}", get(routes::runs::get))
        .route(
            "/runs/{run_id}/events",
            post(routes::events::append).get(routes::events::list),
        )
        .route("/artifacts", post(routes::artifacts::upload))
        .route("/artifacts/{id}", get(routes::artifacts::get))
        .route("/artifacts/{id}/download", get(routes::artifacts::download))
        .route("/runs/{run_id}/artifacts", get(routes::artifacts::list_for_run))
        .route("/health", get(health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
