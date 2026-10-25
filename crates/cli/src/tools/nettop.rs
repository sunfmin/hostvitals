//! Reads one CSV snapshot without name resolution or an interactive display.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-L", "1", "-n", "-P", "-x", "-p", pid]
}
