use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::AppState;

pub fn build(state: AppState) -> Router {
    let protected = Router::new()
        .route("/tool/{tool}", post(crate::tool_proxy::proxy))
        .route("/retrieve", post(crate::retrieval_proxy::retrieve))
        .route("/approval/request", post(crate::approval::request))
        .route("/approval/{id}/decide", post(crate::approval::decide))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            crate::auth::require_bearer,
        ));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> &'static str {
    "ok"
}
