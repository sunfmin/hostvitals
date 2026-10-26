//! Owns sample's arguments and private report lifetime; doctor discards, profile retains.
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::Read,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const REPORT_FLAG: &str = "-file";
pub(crate) const GRACE_SECONDS: u64 = 30;

pub(super) fn args(pid: &str) -> Vec<&str> {
    vec![pid, "1", "10", REPORT_FLAG, "/dev/null"]
}

pub(crate) struct Report {
    pub path: PathBuf,
    pub text: Result<String, String>,
    pub failure: Option<String>,
}

/// Retains the complete report even on a failed sampling exit; stdout is only progress text.
pub(crate) fn read(pid: u32, seconds: u64) -> Result<Report, String> {
    let executable = super::Tool::Sample
        .find()
        .ok_or("executable not found on PATH")?;
    let path = report_path().map_err(|e| format!("cannot create private sample report: {e}"))?;
    let failure = super::execute(
        Command::new(executable)
            .arg(pid.to_string())
            .arg(seconds.to_string())
            .args(["1", "-mayDie", REPORT_FLAG])
            .arg(&path),
        0,
        Duration::from_secs(seconds.saturating_add(GRACE_SECONDS)),
    )
    .err();
    Ok(Report {
        text: read_report(&path),
        path,
        failure,
    })
}

fn report_path() -> std::io::Result<PathBuf> {
    let root = std::env::temp_dir().join("hostvitals");
    match DirBuilder::new().mode(0o700).create(&root) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e),
    }
    let metadata = fs::symlink_metadata(&root)?;
    if !metadata.is_dir()
        || metadata.uid() != crate::direct::host::effective_uid()
        || metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(std::io::Error::other(
            "hostvitals directory must be owned by the caller, be mode 0700, and not be a symlink",
        ));
    }
    let root = std::path::absolute(root)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(std::io::Error::other)?
        .as_nanos();
    for attempt in 0..100 {
        let dir = root.join(format!(
            "profile-{}-{timestamp}-{attempt}",
            std::process::id()
        ));
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {
                let path = dir.join("sample.txt");
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&path)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(
        "cannot allocate a distinct sample report",
    ))
}

fn read_report(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|e| format!("cannot read retained report: {e}"))?;
    let limit = super::MEASUREMENT_LIMIT_MIB * 1024 * 1024;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot read retained report: {e}"))?;
    if bytes.len() > limit {
        return Err(format!("retained sample report exceeded {limit} bytes"));
    }
    String::from_utf8(bytes).map_err(|_| "retained sample report is not UTF-8".into())
}
