//! Each Apple command owns its observation arguments; this module bounds execution and collects diagnostics.
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
pub(crate) mod footprint;
mod fs_usage;
mod iostat;
pub(crate) mod nettop;
mod pmset;
mod powermetrics;
pub(crate) mod ps;
mod sample;
mod top;
pub(crate) mod vmmap;

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
    pub stdout: String,
    pub stdout_readings: Vec<String>,
}

/// Captures a measurement with bounded output and time; truncated reports are never parsed.
pub(crate) fn capture(tool: Tool, args: Vec<&str>) -> Result<String, String> {
    let path = tool.find().ok_or("executable not found on PATH")?;
    execute(
        Command::new(path).args(args),
        MEASUREMENT_LIMIT_MIB * 1024 * 1024,
        MEASUREMENT_TIMEOUT_SECONDS,
    )
    .and_then(|bytes| String::from_utf8(bytes).map_err(|_| "report is not UTF-8".into()))
}

/// Maximum report size; exceeding it skips the read rather than interpreting a partial report.
pub const MEASUREMENT_LIMIT_MIB: usize = 32;

/// Maximum time for each process-memory read, independent of doctor's smaller probes.
pub const MEASUREMENT_TIMEOUT_SECONDS: u64 = 30;

/// Maximum wall-clock duration of one Apple CLI capability check.
pub const PROBE_TIMEOUT_SECONDS: u64 = 8;

/// Bounds execution and diagnostics, retaining snapshot output only when requested.
fn run(command: &mut Command, capture: bool) -> Result<Vec<u8>, String> {
    execute(
        command,
        if capture { 64 * 1024 * 1024 } else { 0 },
        PROBE_TIMEOUT_SECONDS,
    )
}

fn drain(mut pipe: impl std::io::Read, limit: usize) -> std::io::Result<(Vec<u8>, bool)> {
    let mut captured = Vec::new();
    let mut overflow = false;
    let mut buf = [0; 4096];
    loop {
        let n = pipe.read(&mut buf)?;
        if n == 0 {
            break;
        }
        let keep = n.min(limit.saturating_sub(captured.len()));
        captured.extend_from_slice(&buf[..keep]);
        overflow |= keep < n;
    }
    Ok((captured, overflow))
}

fn execute(command: &mut Command, limit: usize, timeout: u64) -> Result<Vec<u8>, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stderr = child.stderr.take().ok_or("no diagnostic pipe")?;
    let stdout = child.stdout.take().ok_or("no observation pipe")?;
    let output_reader = thread::spawn(move || drain(stdout, limit));
    // Drain continuously: a verbose failure must not block the child before the deadline.
    let reader = thread::spawn(move || drain(stderr, 4096));
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if start.elapsed() < Duration::from_secs(timeout) => {
                thread::sleep(Duration::from_millis(10))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!("read exceeded {timeout} seconds"));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(e.to_string());
            }
        }
    };
    let (diagnostic, _) = reader
        .join()
        .map_err(|_| "diagnostic reader failed")?
        .map_err(|e| e.to_string())?;
    let diagnostic = String::from_utf8_lossy(&diagnostic);
    let (output, overflow) = output_reader
        .join()
        .map_err(|_| "observation reader failed")?
        .map_err(|e| e.to_string())?;
    match status {
        Ok(status) if status.success() => {
            if limit != 0 && overflow {
                return Err(format!("report exceeded {limit} bytes"));
            }
            Ok(output)
        }
        Ok(status) => Err(format!("{status}: {diagnostic}")),
        Err(error) => Err(error),
    }
}
