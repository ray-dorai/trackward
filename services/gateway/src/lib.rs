pub mod approval;
pub mod auth;
pub mod config;
pub mod errors;
pub mod ledger_client;
pub mod otel;
pub mod registry;
pub mod retrieval_proxy;
pub mod routes;
pub mod tls;
pub mod tool_proxy;

use std::sync::Arc;

use crate::config::Config;
use crate::ledger_client::LedgerClient;
use crate::registry::ResolvedBinding;

/// Shared state across all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub ledger: LedgerClient,
    pub http: reqwest::Client,
    pub approvals: approval::ApprovalStore,
    /// Cached version IDs resolved from the registry at startup. When empty
    /// (e.g. no registry configured), `bind_if_minted` is a no-op.
    pub binding: ResolvedBinding,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let ledger = LedgerClient::with_optional_tls(
            config.ledger_url.clone(),
            config.service_account.clone(),
            config.ledger_client_tls.as_ref(),
            config.ledger_token.as_deref(),
        )
        .expect("failed to build ledger client (mTLS paths unreadable or token invalid?)");
        Self {
            config: Arc::new(config),
            ledger,
            http: reqwest::Client::new(),
            approvals: approval::ApprovalStore::default(),
            binding: ResolvedBinding::default(),
        }
    }

    pub fn with_binding(mut self, binding: ResolvedBinding) -> Self {
        self.binding = binding;
        self
    }
}

pub fn build_router(state: AppState) -> axum::Router {
    routes::build(state)
}

/// Resolve the registry binding against the ledger (registering versions if
/// needed), cache the IDs in AppState, and build the router. Use this at
/// service startup and in integration tests.
pub async fn build_router_with_registry(mut state: AppState) -> axum::Router {
    match registry::resolve(&state.ledger, &state.config.registry).await {
        Ok(binding) => {
            state.binding = binding;
        }
        Err(e) => {
            // Don't crash the gateway on registry errors — log and proceed
            // without a binding. Operators can see the warning and fix it.
            tracing::error!(error = %e, "failed to resolve registry binding; starting without");
        }
    }
    routes::build(state)
}
