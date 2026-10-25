//! Checks one noninteractive process snapshot without waiting for another window.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-l", "1", "-pid", pid, "-stats", "pid,cpu,mem"]
}
