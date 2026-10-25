//! Checks disk counters in one report and exits.
pub(super) fn args(_pid: &str) -> Vec<&str> {
    vec!["-c", "1"]
}
