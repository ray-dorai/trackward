//! Shared test harness. Starts one ledger process against the docker-compose
//! Postgres/MinIO, one gateway pointing at that ledger, and a mock tool backend
//! that records what it received and returns a scripted response.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use gateway::config::{Config as GatewayConfig, RegistryBinding};
use gateway::{build_router_with_registry, AppState};
use serde_json::Value;
use tokio::sync::Mutex;

/// Mock tool/retrieval backend. Records every incoming request and returns
/// whatever `response` is set to.
#[derive(Clone, Default)]
pub struct MockBackend {
    pub calls: Arc<Mutex<Vec<Value>>>,
    pub response: Arc<Mutex<Value>>,
    pub status: Arc<Mutex<u16>>,
}

impl MockBackend {
    pub fn with_response(body: Value) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            response: Arc::new(Mutex::new(body)),
            status: Arc::new(Mutex::new(200)),
        }
    }

    pub async fn set_status(&self, status: u16) {
        *self.status.lock().await = status;
    }
}

async fn mock_handler(
    State(backend): State<MockBackend>,
    Json(body): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    backend.calls.lock().await.push(body);
    let resp = backend.response.lock().await.clone();
    let code = *backend.status.lock().await;
    (
        axum::http::StatusCode::from_u16(code).unwrap(),
        Json(resp),
    )
}

pub async fn spawn_mock_backend(response: Value) -> (String, MockBackend) {
    let backend = MockBackend::with_response(response);
    let app = axum::Router::new()
        .route("/", axum::routing::post(mock_handler))
        .with_state(backend.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), backend)
}

/// Spawn a real ledger process against the docker-compose Postgres + MinIO.
pub async fn spawn_ledger() -> String {
    // load .env so ledger picks up S3_ENDPOINT + creds
    let _ = dotenvy::from_path("../../.env");
    let _ = dotenvy::from_path(".env");

    let config = ledger::config::Config::from_env();
    let pool = ledger::db::connect(&config).await.expect("ledger db connect");
    let blob_store = ledger::s3::BlobStore::new(&config).await;
    let app = ledger::build_router(ledger::AppState::new(pool, blob_store));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

pub struct GatewayHarness {
    pub url: String,
    pub ledger_url: String,
}

pub async fn spawn_gateway(
    ledger_url: String,
    tool_routes: HashMap<String, String>,
    retrieval_backend: Option<String>,
    gated_tools: Vec<String>,
) -> GatewayHarness {
    spawn_gateway_with_binding(ledger_url, tool_routes, retrieval_backend, gated_tools, None).await
}

pub async fn spawn_gateway_with_binding(
    ledger_url: String,
    tool_routes: HashMap<String, String>,
    retrieval_backend: Option<String>,
    gated_tools: Vec<String>,
    binding: Option<RegistryBinding>,
) -> GatewayHarness {
    let config = GatewayConfig {
        listen_addr: "127.0.0.1:0".into(),
        ledger_url: ledger_url.clone(),
        tool_routes,
        retrieval_backend,
        gated_tools,
        registry: binding.unwrap_or_default(),
    };
    let state = AppState::new(config);
    let app = build_router_with_registry(state).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    GatewayHarness {
        url: format!("http://{addr}"),
        ledger_url,
    }
}

/// Fetch all events for a run from the ledger.
pub async fn get_events(ledger_url: &str, run_id: &str) -> Vec<Value> {
    reqwest::get(format!("{ledger_url}/runs/{run_id}/events"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}
