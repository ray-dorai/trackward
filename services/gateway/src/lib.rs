pub mod approval;
pub mod config;
pub mod errors;
pub mod ledger_client;
pub mod otel;
pub mod retrieval_proxy;
pub mod routes;
pub mod tool_proxy;

use std::sync::Arc;

use crate::config::Config;
use crate::ledger_client::LedgerClient;

/// Shared state across all handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub ledger: LedgerClient,
    pub http: reqwest::Client,
    pub approvals: approval::ApprovalStore,
}

impl AppState {
    pub fn new(config: Config) -> Self {
        let ledger = LedgerClient::new(config.ledger_url.clone());
        Self {
            config: Arc::new(config),
            ledger,
            http: reqwest::Client::new(),
            approvals: approval::ApprovalStore::default(),
        }
    }
}

pub fn build_router(state: AppState) -> axum::Router {
    routes::build(state)
}
