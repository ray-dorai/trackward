pub mod actor;
pub mod anchoring;
pub mod anchors;
pub mod auth;
pub mod chain;
pub mod config;
pub mod db;
pub mod errors;
pub mod hash;
pub mod models;
pub mod otel;
pub mod routes;
pub mod s3;
pub mod signing;
pub mod tls;

use axum::middleware;
use axum::routing::{get, post};
use axum::Router;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

use crate::anchors::AnchorSink;
use crate::s3::BlobStore;
use crate::signing::SigningService;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub blob_store: BlobStore,
    pub signing: SigningService,
    /// Where signed merkle-anchor manifests go. Defaults to
    /// `AnchorSink::Noop` — the anchor row still persists to the DB
    /// but no external WORM upload happens. Production sets this to
    /// an S3 sink via `AppState::with_anchor_sink` after the sink is
    /// constructed from config. Tests inject an in-memory sink so they
    /// can read the uploaded manifest back.
    pub anchor_sink: AnchorSink,
    /// Fallback actor_id used when a write arrives without an
    /// `X-Trackward-Actor` header. `None` makes the header strictly required
    /// (production). `Some(value)` permits legacy/unadorned callers and
    /// stamps their writes with `value`. See `crate::actor` for the full
    /// rationale.
    pub default_actor: Option<String>,
    /// Optional bearer token. When `Some`, every write-path route requires
    /// `Authorization: Bearer <token>`; `None` disables the gate (dev/test
    /// default). Resolved from `LEDGER_AUTH_TOKEN`.
    pub auth_token: Option<String>,
}

impl AppState {
    pub fn new(db: PgPool, blob_store: BlobStore) -> Self {
        Self {
            db,
            blob_store,
            signing: SigningService::from_env(),
            anchor_sink: AnchorSink::Noop,
            default_actor: std::env::var("LEDGER_DEFAULT_ACTOR").ok(),
            auth_token: std::env::var("LEDGER_AUTH_TOKEN").ok().filter(|s| !s.is_empty()),
        }
    }

    /// Override the default_actor resolved from the environment. Test
    /// harnesses use this to either force strict mode (`None`) or pin a
    /// known actor string so assertions don't depend on env state.
    pub fn with_default_actor(mut self, actor: Option<String>) -> Self {
        self.default_actor = actor;
        self
    }

    /// Swap in an anchor sink. Production: `AnchorSink::S3(..)`. Tests:
    /// `AnchorSink::Memory(..)` so the uploaded manifest is readable.
    pub fn with_anchor_sink(mut self, sink: AnchorSink) -> Self {
        self.anchor_sink = sink;
        self
    }

    /// Override the bearer token. Tests use this to exercise both
    /// gated and ungated configurations without touching env vars.
    pub fn with_auth_token(mut self, token: Option<String>) -> Self {
        self.auth_token = token;
        self
    }
}

pub fn build_router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/runs", post(routes::runs::create).get(routes::runs::list))
        .route("/runs/{id}", get(routes::runs::get))
        .route("/runs/{id}/dossier", get(routes::runs::dossier))
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
        .route(
            "/custody-events",
            post(routes::custody_events::create).get(routes::custody_events::list),
        )
        .route(
            "/cases",
            post(routes::cases::create),
        )
        .route("/cases/{id}", get(routes::cases::get))
        .route("/cases/{id}/dossier", get(routes::cases::dossier))
        .route(
            "/cases/{case_id}/evidence",
            post(routes::case_evidence::link).get(routes::case_evidence::list),
        )
        .route(
            "/cases/{case_id}/exports",
            post(routes::export_bundles::create),
        )
        .route("/export-bundles/{id}", get(routes::export_bundles::get))
        .route(
            "/anchors",
            get(routes::anchors::list).post(routes::anchors::trigger),
        )
        .route("/anchors/{id}", get(routes::anchors::get))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_bearer,
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
