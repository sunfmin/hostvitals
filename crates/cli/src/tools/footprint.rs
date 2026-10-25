//! Reads the observer's charged memory without capturing a corpse or report file.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-p", pid]
}

pub(crate) fn read(pid: &str) -> Result<String, String> {
    super::capture(
        super::Tool::Footprint,
        vec!["-p", pid, "-f", "bytes", "--swapped"],
    )
}
