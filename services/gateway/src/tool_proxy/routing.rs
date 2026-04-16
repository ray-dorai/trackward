use std::collections::HashMap;

/// Resolve a tool name to its backend URL.
pub fn resolve<'a>(routes: &'a HashMap<String, String>, tool: &str) -> Option<&'a String> {
    routes.get(tool)
}
