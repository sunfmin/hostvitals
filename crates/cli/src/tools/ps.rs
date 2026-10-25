//! Checks the entitled process-list source against the observer's own process.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-p", pid, "-o", "pid=,pcpu=,rss="]
}
