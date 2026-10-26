//! Checks one noninteractive process snapshot without waiting for another window.
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-l", "1", "-pid", pid, "-stats", "pid,cpu,mem,power"]
}

/// The second sample is top's energy-impact score over this interval, for all users.
pub(crate) fn energy(seconds: u64) -> Result<std::collections::BTreeMap<u32, f64>, String> {
    super::capture_window(
        super::Tool::Top,
        vec!["-l", "2", "-s", &seconds.to_string(), "-stats", "pid,power"],
        seconds,
    )
    .and_then(|text| hostvitals_policy::energy::parse_top(&text).map_err(str::to_owned))
    .map_err(|reason| format!("top: {reason}"))
}
