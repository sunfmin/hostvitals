//! Reads the observer's region summary without producing a memory graph file.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-summary", pid]
}

pub(crate) fn summary(pid: &str) -> Result<String, String> {
    super::capture(super::Tool::Vmmap, args(pid))
}

pub(crate) fn regions(pid: &str) -> Result<String, String> {
    super::capture(super::Tool::Vmmap, vec!["-wide", "-noCoalesce", pid])
}
