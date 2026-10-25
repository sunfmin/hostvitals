//! Reads power-source status; no power-management setting is changed.
pub(super) fn args(_pid: &str) -> Vec<&str> {
    vec!["-g", "batt"]
}
