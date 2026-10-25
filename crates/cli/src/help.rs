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
    /// Measures CPU, memory and network over one window for the host, every user's processes and their Apps.
    ///
    /// Run `hostvitals now --seconds 3 --top 10`. Reads two all-user `ps` and `nettop` snapshots, cumulative host CPU ticks, memory sysctls, process responsibility, and accessible rusage footprints. Only reads; saves nothing. `--seconds` and `--top` require a whole number at least 1. Refusals name the corrected command. Missing or malformed process or host snapshots refuse with a retry line; optional native and network reads are null with a `skipped` entry.
    ///
    /// Answers `source` (`host` or `fixtures`), `window_seconds` (actual monotonic time between process readings), `host`, `processes` (the entire final snapshot), `apps` (totals of those processes), `rankings` and `skipped`. Host CPU covers the same window with minor snapshot collection skew. CPU percentages for processes and Apps use one logical core as 100%, so multithreaded work can exceed 100%. Host percentages cover all cores and total 100%.
    ///
    /// `host.cpu` has `user_percent` (including nice), `system_percent`, `idle_percent`, `logical_cores` and `physical_cores`. `host.load_average` is the 1, 5 and 15 minute load. `host.memory` has `total_bytes`, `free_percent` (kern.memorystatus_level, not a resident-memory subtraction), `pressure` (normal, warn or critical), `swap_used_bytes`, `swap_total_bytes`, and `compressed_bytes` (physical compressed storage). Memory and load are read at the end of the window.
    ///
    /// `host.network` has `window_seconds` (actual monotonic time between the two nettop readings), `bytes_in_per_second` (received), and `bytes_out_per_second` (sent). All network rates divide cumulative-byte changes by this interval; requested --seconds is never used as the divisor. Collection causes minor skew relative to the CPU window. Host network sums nettop's per-process accounting for TCP and UDP on all interfaces, including loopback; proxy or forwarded traffic may be accounted to multiple processes. It is not a physical-link speed. No address or far-end names are requested: nettop runs with -n and process summaries only, so now performs no DNS lookup and sends no network requests. No connection details or remote names are returned.
    ///
    /// Each process has `pid`, `parent_pid`, `uid`, `user`, `name` (executable name, no arguments), `started_at` (ps start time in UTC), `cpu_percent`, `resident_bytes`, `footprint_bytes`, `peak_footprint_bytes` (lifetime peak), and `app_pid` (responsible process). PID plus start time identifies a process. CPU is cumulative-time change over the window, never ps's lifetime percentage. A process absent at the first reading, a reused PID or a regressed counter has null CPU and is counted in `skipped`; an exited process is absent from the final snapshot. ps's hundredth-second CPU precision limits short-window accuracy.
    ///
    /// Each App has `app_pid`, `name`, `started_at`, `process_count`, `cpu_percent`, `resident_bytes`, `footprint_bytes`, and `peak_footprint_bytes`. Totals sum their processes; any missing input makes that total null. Peak is the sum of lifetime process peaks, not a simultaneous App peak. Apps follow macOS responsibility, never a name or parent heuristic. No other responsible process means its own App. An exited responsible process retains its app_pid with null name and started_at; an unreadable responsibility yields null app_pid and is skipped from App totals.
    ///
    /// Every process and App also has `network`, with `bytes_in_per_second` and `bytes_out_per_second`, using host.network.window_seconds. Network includes other users and root processes without elevation. Matching PID and start time is required; a new or reused PID, a process seen by nettop in only one reading, or a regressed byte counter yields null for the affected direction. No sockets in either nettop reading is zero for a process present throughout the window. Short-lived sockets entirely between snapshots may be missed. App rates propagate missing inputs independently for each direction. A missing process change makes that direction of the host rate null, including when a network process exited or could not be identified by ps; checked total overflow also yields null. No partial sum is presented as a complete rate.
    ///
    /// `rankings` has `processes_by_cpu`, `processes_by_memory`, `apps_by_cpu`, `apps_by_memory`: each is up to `--top` full records, descending by cpu_percent or resident_bytes, tied by ascending PID. Null CPU is excluded from CPU rankings. Shared resident memory can be counted in multiple processes, so App resident totals are accounting sums, not unique physical RAM.
    ///
    /// Network rankings are `processes_by_network_in`, `processes_by_network_out`, `apps_by_network_in`, and `apps_by_network_out`. Each contains up to --top full records descending by the corresponding network bytes-per-second field, tied by ascending PID or app_pid. Null values are excluded; measured zeroes remain eligible.
    ///
    /// A missing, failing, oversized or unrecognized nettop report skips network as a whole, retaining CPU and memory. host.network.window_seconds and every network rate are null; network rankings are empty. `skipped` names network with its reason and retry command. Partial byte changes use network.bytes_in_per_second or network.bytes_out_per_second for affected process counts, and host.network equivalents for affected host directions.
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
    /// Explains what one process's memory is made of, without saving reports or changing it.
    ///
    /// Run `hostvitals memory 123 --top 5`. Reads `vmmap -summary`, `vmmap -wide -noCoalesce`, and `footprint -p PID -f bytes --swapped` once each. Each read is bounded to {const:MEASUREMENT_TIMEOUT_SECONDS} seconds and {const:MEASUREMENT_LIMIT_MIB} MiB of output. These are sequential observations of a running process, so they can differ as memory changes. PID must be positive; --top must be positive and defaults to {const:MEMORY_DEFAULT_TOP}.
    ///
    /// Answers one JSON object with `pid`, `top` (requested region limit), `footprint`, `region_kinds`, `malloc_zones`, and `largest_regions`. Every section has `status`: `available` or `skipped`. A skipped section has `reason` and `fix` (the command line to take the read); it has no invented zero or empty reading. Sections fail independently and the command exits zero even if all reads are skipped. Invalid arguments refuse with `error` and nonzero exit.
    ///
    /// Available `footprint` has `bytes`: exact charged memory from footprint's phys_footprint counter, distinct from RAM. The other available sections have `items` arrays. Each region-kind item has `kind` (Apple's region type), `regions` (non-coalesced count), and `memory`. Each malloc-zone item has `zone` (name including its address), `allocations` (allocation count), `allocated_bytes` (live allocations), `regions` (region count), and `memory`. An empty malloc-zone table is an observed empty array.
    ///
    /// Each `memory` object has `virtual_bytes` (address-space size), `resident_bytes` (RAM), `dirty_bytes` (resident modified memory), `swapped_bytes` (swapped or compressed memory), and `clean_bytes` (resident minus dirty). vmmap rounds its text sizes: these byte conversions preserve that reported precision, not exact page accounting. K, M, G and T mean powers of 1024. Clean shared file mappings need not be charged to this process; do not add region totals to obtain Footprint.
    ///
    /// Each largest-region item has `kind`, `address` (start-end range, or kernel-kernel for kernel accounting), `detail` (Apple's permissions, sharing, path or zone text), `memory`, and `dirty_swapped_bytes`. Items follow vmmap's size ranking: dirty plus swapped descending, then virtual size descending, then address ascending. Returns at most `top` items; guard reservations do not outrank memory just because they reserve a large address range.
    ///
    /// Access denied is skipped with `sudo hostvitals memory PID --top N` for the caller to run; hostvitals never invokes sudo. Missing tools include a PATH repair; unrecognized reports include an update command. Exited or protected processes may remain unreadable even as root. Native probe fixtures do not replace memory readings; tests can place Apple CLI stand-ins on PATH.
    #[command(next_help_heading = "Run by the agent", verbatim_doc_comment)]
    Memory {
        /// Positive process ID to observe.
        #[arg(value_parser = clap::value_parser!(u32).range(1..))]
        pid: u32,
        /// Maximum number of largest regions.
        #[arg(long, default_value_t = crate::memory::DEFAULT_TOP, value_parser = clap::value_parser!(u32).range(1..))]
        top: u32,
    },
    /// Opens a command's help, the index, or the whole manual with `hostvitals help all`.
    ///
    /// With no topic, answers the index. `hostvitals help doctor` opens the doctor's help. An unknown topic refuses with a line to open the index.
    #[command(next_help_heading = "Run by the agent", verbatim_doc_comment)]
    Help {
        /// `doctor`, `now`, or `memory` for a command, `all` for the full manual, or omit for the index.
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
        .constant(
            "MEASUREMENT_TIMEOUT_SECONDS",
            crate::tools::MEASUREMENT_TIMEOUT_SECONDS,
        )
        .constant("MEASUREMENT_LIMIT_MIB", crate::tools::MEASUREMENT_LIMIT_MIB)
        .constant("MEMORY_DEFAULT_TOP", crate::memory::DEFAULT_TOP)
}
#[cfg(test)]
mod tests {
    #[test]
    fn manual_is_whole() {
        agent_help::check(&super::manual()).unwrap();
    }
}
