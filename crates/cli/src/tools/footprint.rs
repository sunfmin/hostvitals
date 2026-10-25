//! Reads the observer's charged memory without capturing a corpse or report file.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-p", pid]
}
