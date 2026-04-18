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
    /// Bearer token required on the gateway's own tool/retrieve/approval
    /// endpoints. None disables the check (dev default).
    pub auth_token: Option<String>,
    /// Bearer token the gateway presents when talking to the ledger. None
    /// means no Authorization header is sent (works against an unlocked ledger).
    pub ledger_token: Option<String>,
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
            auth_token: env::var("GATEWAY_AUTH_TOKEN").ok().filter(|s| !s.is_empty()),
            ledger_token: env::var("LEDGER_CLIENT_TOKEN").ok().filter(|s| !s.is_empty()),
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
