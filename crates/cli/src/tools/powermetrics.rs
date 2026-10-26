//! Reads one short component-power sample, only after the root policy permits it.
pub(super) fn args(_pid: &str) -> Vec<&str> {
    vec!["-n", "1", "-i", "100", "--samplers", "cpu_power"]
}

/// Called only for an already-root observer; no sudo or privilege escalation occurs here.
pub(crate) fn gpu_times(seconds: u64) -> Result<hostvitals_policy::energy::GpuTimes, String> {
    let interval = seconds
        .checked_mul(1000)
        .filter(|n| *n <= i32::MAX as u64)
        .ok_or("powermetrics interval exceeds its supported millisecond range")?
        .to_string();
    super::capture_window(
        super::Tool::Powermetrics,
        vec![
            "-n",
            "1",
            "-i",
            &interval,
            "--samplers",
            "tasks",
            "--show-process-gpu",
        ],
        seconds,
    )
    .and_then(|text| hostvitals_policy::energy::parse_gpu_times(&text).map_err(str::to_owned))
    .map_err(|reason| format!("powermetrics tasks: {reason}"))
}
