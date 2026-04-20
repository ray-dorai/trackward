use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Config {
    pub listen_addr: String,
    pub ledger_url: String,
    /// Map of tool name → backend URL (e.g. "bash" → "http://bash-tool.svc:8080/exec").
    /// Read from TOOL_ROUTES env as `name1=url1,name2=url2`.
    pub tool_routes: HashMap<String, String>,
    /// URL of the retrieval backend. Single backend for Phase 2; extend later.
    pub retrieval_backend: Option<String>,
    /// Tools that require human approval before proxying.
    pub gated_tools: Vec<String>,
    /// Registry binding — which prompt/policy/eval this gateway stamps runs with.
    pub registry: RegistryBinding,
    /// Identifier this gateway uses when calling the ledger. Sent on every
    /// ledger write as `X-Trackward-Actor`. In production this should be a
    /// service-account name tied to the deployment (e.g.
    /// `gateway/prod-us-east/v1`); in tests it defaults to `gateway-test`.
    /// Read from `GATEWAY_SERVICE_ACCOUNT`.
    pub service_account: String,
    /// Server-side mTLS paths (what clients of the gateway present). All or
    /// nothing; partial configs are logged and ignored. See
    /// `ledger::config::TlsPaths` for the same pattern on the ledger side.
    pub tls: Option<TlsPaths>,
    /// Client-side mTLS paths (what the gateway presents when calling the
    /// ledger). Independent of `tls` because the two endpoints may be run
    /// by different operators with different PKI. All three of
    /// `LEDGER_CLIENT_CERT_PATH`, `LEDGER_CLIENT_KEY_PATH`,
    /// `LEDGER_SERVER_CA_PATH` must be set to enable.
    pub ledger_client_tls: Option<LedgerClientTls>,
}

#[derive(Clone, Debug)]
pub struct TlsPaths {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub client_ca_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct LedgerClientTls {
    /// PEM: gateway's leaf cert (+ intermediates) presented to the ledger.
    pub cert_path: PathBuf,
    /// PEM: gateway's private key matching `cert_path`.
    pub key_path: PathBuf,
    /// PEM: CA bundle used to verify the ledger's server cert.
    pub server_ca_path: PathBuf,
}

/// Which prompt & policy (and, by extension, eval) version every run minted
/// by this gateway gets tied to. All fields are optional; a gateway with an
/// empty binding simply doesn't stamp runs (useful in tests and bring-up).
#[derive(Clone, Debug, Default)]
pub struct RegistryBinding {
    pub registry_dir: Option<PathBuf>,
    pub prompt_workflow: Option<String>,
    pub prompt_version: Option<String>,
    pub policy_scope: Option<String>,
    pub policy_version: Option<String>,
    pub git_sha: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let tool_routes = env::var("TOOL_ROUTES")
            .ok()
            .map(parse_routes)
            .unwrap_or_default();

        let gated_tools = env::var("GATED_TOOLS")
            .ok()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_default();

        let registry = RegistryBinding {
            registry_dir: env::var("REGISTRY_DIR").ok().map(PathBuf::from),
            prompt_workflow: env::var("PROMPT_WORKFLOW").ok(),
            prompt_version: env::var("PROMPT_VERSION").ok(),
            policy_scope: env::var("POLICY_SCOPE").ok(),
            policy_version: env::var("POLICY_VERSION").ok(),
            git_sha: env::var("GIT_SHA").ok(),
        };

        Self {
            listen_addr: env::var("GATEWAY_LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:4000".into()),
            ledger_url: env::var("LEDGER_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            tool_routes,
            retrieval_backend: env::var("RETRIEVAL_BACKEND").ok(),
            gated_tools,
            registry,
            service_account: env::var("GATEWAY_SERVICE_ACCOUNT")
                .unwrap_or_else(|_| "gateway".into()),
            tls: TlsPaths::from_env(),
            ledger_client_tls: LedgerClientTls::from_env(),
        }
    }

    pub fn tls_enabled(&self) -> bool {
        self.tls.is_some()
    }
}

impl TlsPaths {
    pub fn from_env() -> Option<Self> {
        let cert = env::var("TLS_CERT_PATH").ok();
        let key = env::var("TLS_KEY_PATH").ok();
        let ca = env::var("TLS_CLIENT_CA_PATH").ok();
        match (cert, key, ca) {
            (Some(cert), Some(key), Some(ca)) => Some(Self {
                cert_path: PathBuf::from(cert),
                key_path: PathBuf::from(key),
                client_ca_path: PathBuf::from(ca),
            }),
            (None, None, None) => None,
            (cert, key, ca) => {
                tracing::warn!(
                    cert = cert.is_some(),
                    key = key.is_some(),
                    ca = ca.is_some(),
                    "TLS_CERT_PATH / TLS_KEY_PATH / TLS_CLIENT_CA_PATH must all be set or all unset; starting in plaintext mode"
                );
                None
            }
        }
    }
}

impl LedgerClientTls {
    pub fn from_env() -> Option<Self> {
        let cert = env::var("LEDGER_CLIENT_CERT_PATH").ok();
        let key = env::var("LEDGER_CLIENT_KEY_PATH").ok();
        let ca = env::var("LEDGER_SERVER_CA_PATH").ok();
        match (cert, key, ca) {
            (Some(cert), Some(key), Some(ca)) => Some(Self {
                cert_path: PathBuf::from(cert),
                key_path: PathBuf::from(key),
                server_ca_path: PathBuf::from(ca),
            }),
            (None, None, None) => None,
            (cert, key, ca) => {
                tracing::warn!(
                    cert = cert.is_some(),
                    key = key.is_some(),
                    ca = ca.is_some(),
                    "LEDGER_CLIENT_CERT_PATH / LEDGER_CLIENT_KEY_PATH / LEDGER_SERVER_CA_PATH must all be set or all unset; gateway will call ledger without client auth"
                );
                None
            }
        }
    }
}

fn parse_routes(s: String) -> HashMap<String, String> {
    s.split(',')
        .filter_map(|entry| {
            let (name, url) = entry.split_once('=')?;
            Some((name.trim().to_string(), url.trim().to_string()))
        })
        .collect()
}
