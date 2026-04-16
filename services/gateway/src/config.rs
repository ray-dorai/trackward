use std::collections::HashMap;
use std::env;

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

        Self {
            listen_addr: env::var("GATEWAY_LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:4000".into()),
            ledger_url: env::var("LEDGER_URL")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            tool_routes,
            retrieval_backend: env::var("RETRIEVAL_BACKEND").ok(),
            gated_tools,
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
