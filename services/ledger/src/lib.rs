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
        .route(
            "/prompt-versions",
            post(routes::prompt_versions::create).get(routes::prompt_versions::list),
        )
        .route("/prompt-versions/{id}", get(routes::prompt_versions::get))
        .route(
            "/policy-versions",
            post(routes::policy_versions::create).get(routes::policy_versions::list),
        )
        .route("/policy-versions/{id}", get(routes::policy_versions::get))
        .route(
            "/eval-results",
            post(routes::eval_results::create).get(routes::eval_results::list),
        )
        .route("/eval-results/{id}", get(routes::eval_results::get))
        .route(
            "/runs/{run_id}/bindings",
            post(routes::run_bindings::create).get(routes::run_bindings::get),
        )
        .route(
            "/tool-invocations",
            post(routes::tool_invocations::create),
        )
        .route(
            "/tool-invocations/{id}",
            get(routes::tool_invocations::get),
        )
        .route(
            "/runs/{run_id}/tool-invocations",
            get(routes::tool_invocations::list_for_run),
        )
        .route("/side-effects", post(routes::side_effects::create))
        .route("/side-effects/{id}", get(routes::side_effects::get))
        .route(
            "/runs/{run_id}/side-effects",
            get(routes::side_effects::list_for_run),
        )
        .route("/guardrails", post(routes::guardrails::create))
        .route("/guardrails/{id}", get(routes::guardrails::get))
        .route(
            "/runs/{run_id}/guardrails",
            get(routes::guardrails::list_for_run),
        )
        .route("/human-approvals", post(routes::human_approvals::create))
        .route(
            "/human-approvals/{id}",
            get(routes::human_approvals::get),
        )
        .route("/bias-slices", post(routes::bias_slices::create))
        .route("/bias-slices/{id}", get(routes::bias_slices::get))
        .route(
            "/runs/{run_id}/bias-slices",
            get(routes::bias_slices::list_for_run),
        )
        .route("/health", get(health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
