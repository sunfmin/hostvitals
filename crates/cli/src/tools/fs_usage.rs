//! Reads one second of the observer's filesystem activity, only when already root.
pub(super) fn args<'a>(pid: &'a str, seconds: &'a str) -> Vec<&'a str> {
    vec!["-w", "-t", seconds, "-f", "filesys", pid]
}

/// Runs in parallel with the counter window and is always joined before returning an answer.
pub fn read(
    pid: u32,
    seconds: u64,
    initial_files: &[hostvitals_policy::files::File],
) -> Result<Vec<hostvitals_policy::files::Activity>, String> {
    let path = super::Tool::FsUsage
        .find()
        .ok_or("fs_usage missing on PATH")?;
    let bytes = super::execute(
        std::process::Command::new(path).args(args(&pid.to_string(), &seconds.to_string())),
        super::MEASUREMENT_LIMIT_MIB * 1024 * 1024,
        std::time::Duration::from_secs(seconds.saturating_add(5)),
    )?;
    if !bytes.stderr.trim().is_empty() {
        return Err("fs_usage reported diagnostics; event stream may be incomplete".into());
    }
    let text = String::from_utf8(bytes.stdout).map_err(|_| "fs_usage report is not UTF-8")?;
    hostvitals_policy::files::activity(&text, initial_files).map_err(str::to_owned)
}
