//! The clap command tree and its doc comments are the manual's one home.
use crate::tools::Tool;
use agent_help::Help;
use clap::{CommandFactory, Parser, Subcommand};

/// Observes a Mac's CPU, memory, disk, network and energy, read-only.
///
/// Runs on macOS only, on arm64 and x86_64. Run `hostvitals doctor` first to see which measurements can be read here. Every command only reads; none changes anything. The commands themselves show what can be observed, so there is no separate dry-run mode.
///
/// Never runs `sudo` itself, installs anything or prompts. A measurement needing root is listed as `skipped`, with the line that takes it; the caller decides whether to run that line. No daemon, saved history or cache: each invocation observes one window and exits.
///
/// Commands answer exactly one JSON object on stdout. A refusal is `{"error":"reason; fix command"}` with a nonzero exit code. Missing or skipped capabilities are expected answers and exit zero. Warnings go to stderr. Exceptions: `--help`, `-h`, and `help` answer Markdown; `--version` answers a version line.
#[derive(Parser)]
#[command(name = "hostvitals", version = env!("HOSTVITALS_VERSION"), verbatim_doc_comment, disable_help_subcommand = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Measures CPU and memory over one window for the host, every user's processes and their Apps.
    ///
    /// Run `hostvitals now --seconds 3 --top 10`. Reads two all-user `ps` snapshots, cumulative host CPU ticks, memory sysctls, process responsibility, and accessible rusage footprints. Only reads; saves nothing. `--seconds` and `--top` require a whole number at least 1. Refusals name the corrected command. Missing or malformed process or host snapshots refuse with a retry line; optional native reads are null with a `skipped` entry.
    ///
    /// Answers `source` (`host` or `fixtures`), `window_seconds` (actual monotonic time between process readings), `host`, `processes` (the entire final snapshot), `apps` (totals of those processes), `rankings` and `skipped`. Host CPU covers the same window with minor snapshot collection skew. CPU percentages for processes and Apps use one logical core as 100%, so multithreaded work can exceed 100%. Host percentages cover all cores and total 100%.
    ///
    /// `host.cpu` has `user_percent` (including nice), `system_percent`, `idle_percent`, `logical_cores` and `physical_cores`. `host.load_average` is the 1, 5 and 15 minute load. `host.memory` has `total_bytes`, `free_percent` (kern.memorystatus_level, not a resident-memory subtraction), `pressure` (normal, warn or critical), `swap_used_bytes`, `swap_total_bytes`, and `compressed_bytes` (physical compressed storage). Memory and load are read at the end of the window.
    ///
    /// Each process has `pid`, `parent_pid`, `uid`, `user`, `name` (executable name, no arguments), `started_at` (ps start time in UTC), `cpu_percent`, `resident_bytes`, `footprint_bytes`, `peak_footprint_bytes` (lifetime peak), and `app_pid` (responsible process). PID plus start time identifies a process. CPU is cumulative-time change over the window, never ps's lifetime percentage. A process absent at the first reading, a reused PID or a regressed counter has null CPU and is counted in `skipped`; an exited process is absent from the final snapshot. ps's hundredth-second CPU precision limits short-window accuracy.
    ///
    /// Each App has `app_pid`, `name`, `started_at`, `process_count`, `cpu_percent`, `resident_bytes`, `footprint_bytes`, and `peak_footprint_bytes`. Totals sum their processes; any missing input makes that total null. Peak is the sum of lifetime process peaks, not a simultaneous App peak. Apps follow macOS responsibility, never a name or parent heuristic. No other responsible process means its own App. An exited responsible process retains its app_pid with null name and started_at; an unreadable responsibility yields null app_pid and is skipped from App totals.
    ///
    /// `rankings` has `processes_by_cpu`, `processes_by_memory`, `apps_by_cpu`, `apps_by_memory`: each is up to `--top` full records, descending by cpu_percent or resident_bytes, tied by ascending PID. Null CPU is excluded from CPU rankings. Shared resident memory can be counted in multiple processes, so App resident totals are accounting sums, not unique physical RAM.
    ///
    /// `skipped` entries have `measurement`, `count` (affected processes or host fields), `reason`, and `fix`. Without root, other users' footprints are null and get one combined entry with `sudo hostvitals now`; their CPU and resident memory remain readable and ranked. Own-process rusage failures are separately aggregated. A missing private responsibility interface needs an updated hostvitals, rather than invented App attribution. Explicit HOSTVITALS_PROBE_FIXTURES native fixtures are labelled source fixtures and warned on stderr; missing fixtures never fall back to live reads.
    #[command(next_help_heading = "Run by the agent", verbatim_doc_comment)]
    Now {
        /// Minimum seconds in the observation window; a whole number at least 1.
        #[arg(long, default_value = "3", allow_hyphen_values = true, value_parser = seconds)]
        seconds: std::num::NonZeroU64,
        /// Maximum records in each ranking; a whole number at least 1.
        #[arg(long, default_value = "10", allow_hyphen_values = true, value_parser = top)]
        top: std::num::NonZeroUsize,
    },
    /// Checks every observation source, reads the host, and changes nothing.
    ///
    /// Run this first, after a macOS update, or when a measurement is unavailable. Reads the Apple CLIs below with bounded, noninteractive probes. Process probes target this invocation, never an arbitrary process. Native reads check the observer's task information and cumulative counters, an actual IOReport Energy Model subscription and sample, and its responsible App. No report files are saved. The probes are independent checks, not one simultaneous resource measurement.
    ///
    /// Answers `{"source":"host","host":{"macos_version":"27.0.1","arch":"arm64","is_root":false},"checks":[...]}`. `source` is `fixtures` only when the explicit testing environment `HOSTVITALS_PROBE_FIXTURES` supplies native observations. `host.macos_version` is the product version; `arch` is the binary's arm64 or x86_64 architecture; `is_root` describes its effective user.
    ///
    /// Each check has `name`, `kind` (`cli` or `direct`), and `status` (`available`, `unavailable`, or `skipped`). An unavailable or skipped check also has `reason` and `fix`, a command to retry or enable the read; `guidance`, when present, explains an interface change. A missing executable uses a PATH repair. A CLI failure includes its exit status and bounded stderr. A probe is stopped after {const:PROBE_TIMEOUT_SECONDS} seconds. Private interfaces can change: they are reported unavailable, never substituted with invented readings. An unavailable private interface may need a newer hostvitals; root does not repair a missing interface.
    ///
    /// Available direct checks also have `data`. `rusage`: `resident_bytes` (RAM), `footprint_bytes` (charged memory), `peak_footprint_bytes` (lifetime peak), `read_bytes` and `written_bytes` (cumulative disk I/O), and `energy_nj` (cumulative energy in nanojoules), for this process. `libproc`: `pid`, `resident_bytes`, and `threads` for this process. `ioreport`: `energy_channels` discovered and `sampled_channels` actually read, both channel counts, not watts. `responsibility`: this process's `pid` and responsible `app_pid`; a process with no other responsible process is its own App.
    ///
    /// Root-only probes are skipped without executing them. To take those reads, run `sudo hostvitals doctor` yourself. It never asks for credentials. If a CLI probe is denied, run its check again as root yourself; if the interface is unavailable on this macOS version, retry after updating hostvitals. All findings are returned together with exit zero; only an invalid invocation or unreadable host identity refuses the command.
    ///
    /// Apple CLIs checked:
    ///
    /// {table:tools}
    #[command(next_help_heading = "Run by the agent", verbatim_doc_comment)]
    Doctor,
    /// Opens a command's help, the index, or the whole manual with `hostvitals help all`.
    ///
    /// With no topic, answers the index. `hostvitals help doctor` opens the doctor's help. An unknown topic refuses with a line to open the index.
    #[command(next_help_heading = "Run by the agent", verbatim_doc_comment)]
    Help {
        /// `doctor` or `now` for a command, `all` for the full manual, or omit for the index.
        #[arg(verbatim_doc_comment)]
        topic: Option<String>,
    },
}

fn positive(value: &str, flag: &str) -> Result<u64, String> {
    if !value.is_empty()
        && value.bytes().all(|b| b.is_ascii_digit())
        && let Ok(n @ 1..) = value.parse()
    {
        return Ok(n);
    }
    Err(format!(
        "{flag} requires a whole number at least 1; run hostvitals now {flag} 1"
    ))
}
fn seconds(value: &str) -> Result<std::num::NonZeroU64, String> {
    positive(value, "--seconds").map(|n| std::num::NonZeroU64::new(n).unwrap())
}
fn top(value: &str) -> Result<std::num::NonZeroUsize, String> {
    positive(value, "--top").map(|n| std::num::NonZeroUsize::new(n as usize).unwrap())
}

/// Assembles help directly from the command registry and the capability registry.
pub(crate) fn manual() -> Help {
    Help::new(Cli::command())
        .table::<Tool>("tools")
        .constant("PROBE_TIMEOUT_SECONDS", crate::tools::PROBE_TIMEOUT_SECONDS)
}
#[cfg(test)]
mod tests {
    #[test]
    fn manual_is_whole() {
        agent_help::check(&super::manual()).unwrap();
    }
}
