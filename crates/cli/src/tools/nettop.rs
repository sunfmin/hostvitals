//! Reads one CSV snapshot without name resolution or an interactive display.
use hostvitals_policy::network;
use std::collections::BTreeMap;

pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-L", "1", "-n", "-P", "-x", "-p", pid]
}

/// Reads every user's process summaries; never resolves addresses or fetches connection names.
pub(crate) fn snapshot() -> Result<BTreeMap<u32, network::Counters>, String> {
    super::capture(
        super::Tool::Nettop,
        vec!["-L", "1", "-n", "-P", "-x", "-J", "bytes_in,bytes_out"],
    )
    .and_then(|text| network::parse(&text).map_err(str::to_owned))
    .map_err(|reason| format!("nettop: {reason}"))
}
