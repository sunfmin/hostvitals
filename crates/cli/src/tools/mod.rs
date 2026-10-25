//! Each Apple command owns its observation arguments; this module bounds execution and collects diagnostics.
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
mod footprint;
mod fs_usage;
mod iostat;
mod nettop;
mod pmset;
mod powermetrics;
pub(crate) mod ps;
mod sample;
mod top;
mod vmmap;

/// The closed set of Apple CLIs doctor probes, shared with the CLI stand-in.
#[derive(Clone, Copy, clap::ValueEnum, agent_help::Help)]
pub enum Tool {
    /// Process CPU and resident memory; found on PATH.
    Ps,
    /// Entitled process CPU and memory snapshots; found on PATH.
    Top,
    /// Per-process network counters; found on PATH.
    Nettop,
    /// Disk I/O counters; found on PATH.
    Iostat,
    /// Entitled call-stack sampling; found on PATH.
    Sample,
    /// Entitled memory-region inspection; found on PATH.
    Vmmap,
    /// Entitled charged-memory inspection; found on PATH.
    Footprint,
    /// Power-source and energy status; found on PATH.
    Pmset,
    /// Component power; requires root and is otherwise skipped.
    Powermetrics,
    /// Filesystem activity; requires root and is otherwise skipped.
    FsUsage,
}
impl Tool {
    /// Every declared tool, derived from the enum rather than a second registry.
    pub fn all() -> &'static [Self] {
        <Self as clap::ValueEnum>::value_variants()
    }
    /// The executable spelling is the help table's snake_case identifier.
    pub fn name(self) -> &'static str {
        agent_help::Table::entry(&self).name
    }
    pub fn needs_root(self) -> bool {
        matches!(self, Self::Powermetrics | Self::FsUsage)
    }
    pub fn find(self) -> Option<PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        env::split_paths(&env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(self.name()))
            .find(|path| {
                fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
    }
    /// Executes a bounded read against the current process (or host-wide counters).
    pub fn probe(self, path: PathBuf) -> Result<(), String> {
        let pid = std::process::id().to_string();
        let args = match self {
            Self::Ps => ps::args(&pid),
            Self::Top => top::args(&pid),
            Self::Nettop => nettop::args(&pid),
            Self::Iostat => iostat::args(&pid),
            Self::Sample => sample::args(&pid),
            Self::Vmmap => vmmap::args(&pid),
            Self::Footprint => footprint::args(&pid),
            Self::Pmset => pmset::args(&pid),
            Self::Powermetrics => powermetrics::args(&pid),
            Self::FsUsage => fs_usage::args(&pid),
        };
        run(Command::new(path).args(args), false).map(|_| ())
    }
}

/// Stand-ins use one JSON file per invocation, with call recording and failure injection.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StandIn {
    pub exit_code: i32,
    pub stderr: String,
    pub calls: Vec<Vec<String>>,
    pub delay_ms: u64,
    pub stdout_readings: Vec<String>,
}

/// Maximum wall-clock duration of one Apple CLI capability check.
pub const PROBE_TIMEOUT_SECONDS: u64 = 8;

/// Bounds execution and diagnostics, retaining snapshot output only when requested.
fn run(command: &mut Command, capture: bool) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stderr = child.stderr.take().ok_or("no diagnostic pipe")?;
    let stdout = child.stdout.take();
    let output = thread::spawn(move || {
        let mut captured = Vec::new();
        let mut overflow = false;
        if let Some(mut stdout) = stdout {
            let mut buf = [0; 8192];
            loop {
                let n = stdout.read(&mut buf).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                let keep = n.min((64 * 1024 * 1024usize).saturating_sub(captured.len()));
                overflow |= keep < n;
                captured.extend_from_slice(&buf[..keep]);
            }
        }
        if overflow {
            Err("process snapshot exceeded 64 MiB".to_string())
        } else {
            Ok(captured)
        }
    });
    // Drain continuously: a verbose failure must not block the child before the deadline.
    let reader = thread::spawn(move || {
        let mut captured = Vec::new();
        let mut stderr = stderr;
        let mut buf = [0; 4096];
        while let Ok(n) = stderr.read(&mut buf) {
            if n == 0 {
                break;
            }
            let keep = n.min(4096usize.saturating_sub(captured.len()));
            captured.extend_from_slice(&buf[..keep]);
        }
        String::from_utf8_lossy(&captured).trim().to_string()
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < Duration::from_secs(PROBE_TIMEOUT_SECONDS) => {
                thread::sleep(Duration::from_millis(10))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("probe exceeded {PROBE_TIMEOUT_SECONDS} seconds"));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    let diagnostic = reader
        .join()
        .unwrap_or_else(|_| "diagnostic reader failed".into());
    let output = output.join().map_err(|_| "observation reader failed")?;
    match status {
        Ok(status) if status.success() => output,
        Ok(status) => Err(format!("{status}: {diagnostic}")),
        Err(error) => Err(error),
    }
}
