/// Return true if a given tool name is behind an approval gate.
pub fn is_gated(gated_tools: &[String], tool: &str) -> bool {
    gated_tools.iter().any(|g| g == tool)
}
