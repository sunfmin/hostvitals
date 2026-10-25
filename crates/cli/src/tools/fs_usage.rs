//! Reads one second of the observer's filesystem activity, only when already root.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-w", "-t", "1", "-f", "filesys", pid]
}
