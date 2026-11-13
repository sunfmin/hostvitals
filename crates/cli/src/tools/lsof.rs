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
    if !only_recovered_mounts(&output.stderr) {
        return Err("lsof reported diagnostics; file inventory may be incomplete".into());
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "lsof report is not UTF-8")?;
    hostvitals_policy::files::parse(&text, pid).map_err(str::to_owned)
}

/// Whether lsof's diagnostics say only that it could not stat() a mounted file
/// system and took its device from the mount table instead, as it does for a
/// mount another user owns (Xcode's CoreDevice devicefs). The files of the
/// inspected Process are still listed, by path, so the inventory stays
/// complete; any other diagnostic leaves it unknown.
fn only_recovered_mounts(stderr: &str) -> bool {
    let mut lines = stderr.lines().map(str::trim).filter(|l| !l.is_empty());
    while let Some(line) = lines.next() {
        let recovered = line.starts_with("lsof: WARNING: can't stat() ")
            && lines.next() == Some("Output information may be incomplete.")
            && lines.next().is_some_and(|l| {
                l.starts_with("assuming \"dev=") && l.ends_with("\" from mount table")
            });
        if !recovered {
            return false;
        }
    }
    true
}
