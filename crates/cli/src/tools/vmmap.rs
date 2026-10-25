//! Reads the observer's region summary without producing a memory graph file.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-summary", pid]
}
