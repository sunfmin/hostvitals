//! Samples the observer for one second and sends the report to a discard device, never a file.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec![pid, "1", "10", "-file", "/dev/null"]
}
