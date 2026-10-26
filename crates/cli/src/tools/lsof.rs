//! lsof's NUL-delimited fields keep spaces and numeric endpoints intact.
use super::Tool;
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-nP", "-a", "-p", pid, "-F0pftPnT"]
}
pub fn read(pid: u32) -> Result<hostvitals_policy::files::Inventory, String> {
    let path = Tool::Lsof.find().ok_or("lsof missing on PATH")?;
    let output = super::execute(
        std::process::Command::new(path).args(args(&pid.to_string())),
        super::MEASUREMENT_LIMIT_MIB * 1024 * 1024,
        std::time::Duration::from_secs(super::MEASUREMENT_TIMEOUT_SECONDS),
    )?;
    if !output.stderr.trim().is_empty() {
        return Err("lsof reported diagnostics; file inventory may be incomplete".into());
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "lsof report is not UTF-8")?;
    hostvitals_policy::files::parse(&text, pid).map_err(str::to_owned)
}
