//! The clap command tree and its doc comments are the manual's one home.
use crate::tools::Tool;
use agent_help::Help;
use clap::{CommandFactory, Parser, Subcommand};

/// Observes a Mac's CPU, memory, disk, network and energy, read-only.
///
/// Runs on macOS only, on arm64 and x86_64. Run `hostvitals doctor` first to see which measurements can be read here. Every command only observes the Process; none changes its behaviour. Profile writes its raw sample report under $TMPDIR/hostvitals/ for the caller to read and remove; other commands save no reports. The commands themselves show what can be observed, so there is no separate dry-run mode.
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
    /// Samples one Process's call stacks and retains its full sample report.
    ///
    /// Run `hostvitals profile 123 --seconds 5 --top 10`. Uses Apple's entitled sample tool; never controls the Process or runs sudo. PID is 1..2147483647. --seconds and --top are positive whole numbers, defaulting to {const:PROFILE_DEFAULT_SECONDS} and {const:PROFILE_DEFAULT_TOP}. The sampling interval is one millisecond.
    ///
    /// Answers `pid`, `seconds` (requested Window, not measured elapsed time), `top` (limit per ranking), `status` (available or skipped), and `report_path` (retained full report, or null if none). Available answers also have `total_samples`, `waiting_samples`, `mostly_waiting`, `observation` (plain-language summary), `partial`, `diagnostic` (sample's bounded failure reason, otherwise null), `hot_paths`, and `top_frames`.
    ///
    /// Each hot path has `frames` (root to leaf, including image names), `sample_count` (exclusive count), and `share` (fraction of all thread samples, 0..1). Matching paths are combined across threads. Top-frame items have `frame` and `sample_count`, combining exclusive leaf counts across all threads, including waits and recursion without double counting. Rankings descend by count and break ties lexically. They include small counts omitted by sample's thresholded tables. Counts describe thread samples, not CPU percentages or elapsed time; sleeping threads count too.
    ///
    /// `mostly_waiting` means more than half the samples end in recognized kernel wait, sleep, lock or event symbols. Such paths are excluded from hot_paths but remain in top_frames and the share denominator. Other kernel calls are not assumed to be waits. A missing graph footer or failed sample exit marks useful observations partial; a Process ending during sampling still returns its observed stacks, even when sample exits unsuccessfully. A completed report can cover less than the requested Window.
    ///
    /// Writes a distinct owner-only report (0600) in a private directory (0700) under $TMPDIR/hostvitals/ (the system temporary directory when TMPDIR is unset). Summarizes those same retained bytes, never sample's progress output. The caller can read report_path after exit and should remove its containing profile directory when finished. Temporary storage can be purged by macOS; it is not durable history. Doctor still discards its sample report. An unsafe existing hostvitals directory is refused rather than reused.
    ///
    /// Skipped answers have `reason` and `fix`, with no invented zero counts or rankings, and exit zero. Permission denial names `sudo hostvitals profile PID --seconds N --top N` for the caller; protected Processes may remain unreadable as root. Missing sample names a PATH repair; malformed reports name an update. Retained but unreadable or malformed reports still have report_path. Reports over {const:MEASUREMENT_LIMIT_MIB} MiB are retained but skipped; sample execution is bounded to the requested seconds plus {const:PROFILE_GRACE_SECONDS} seconds. Invalid arguments refuse with error and a help command.
    #[command(next_help_heading = "Run by the agent", verbatim_doc_comment)]
    Profile {
        /// Positive process ID to observe.
        #[arg(value_parser = clap::value_parser!(u32).range(1..=2147483647))]
        pid: u32,
        /// Requested sampling Window in whole seconds.
        #[arg(long, default_value_t = crate::profile::DEFAULT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
        seconds: u64,
        /// Maximum entries in each ranking.
        #[arg(long, default_value_t = crate::profile::DEFAULT_TOP, value_parser = clap::value_parser!(u32).range(1..))]
        top: u32,
    },
    /// Observes one process whole, with its command arguments redacted.
    ///
    /// Run `hostvitals process 123 --seconds 3`. Reads two ps inventories and accessible native counters, exact KERN_PROCARGS2 arguments, task thread count, responsible App and lsof files/sockets. Root also runs fs_usage over the window. Only observes; saves nothing. PID is 1..2147483647 and --seconds is a positive whole number (default {const:PROCESS_DEFAULT_SECONDS}). A missing, exited or replaced PID refuses with its PID and a retry line; mandatory ps failure also refuses.
    ///
    /// Answers `source` (host or explicitly labelled fixtures), `pid`, `parent_pid`, `uid`, `user`, `name` (executable basename), `started_at` (UTC ps start time), `app`, `command_line`, `cwd`, `cpu_seconds` (cumulative), `cpu_percent` (window change; one core is 100%), `window_seconds` (actual monotonic CPU interval), `resident_bytes`, `footprint_bytes`, `peak_footprint_bytes` (lifetime), `disk`, `energy_nj` (cumulative nanojoules), `threads`, `open_files`, `sockets`, `file_activity`, and `skipped`.
    ///
    /// `app` has `app_pid`, `name` and `started_at`; unreadable/exited App identity leaves its latter fields null. Attribution uses macOS responsibility, never the parent. `command_line` is an array preserving argv boundaries, empty arguments and public values; invalid UTF-8 uses replacement characters. A flag containing {const:SECRET_FLAG_WORDS} (any case) has its equals-form or next-argument value replaced by <redacted>, even when empty or flag-like. URL userinfo is replaced by <redacted>. This is flag/URL redaction, not detection of secrets inside arbitrary scripts or public positional text.
    ///
    /// hostvitals asks for process arguments and parses only the argc-delimited vector. It never interprets, returns, logs or keeps any environment entry. The kernel buffer is bounded to {const:ARGUMENT_LIMIT_MIB} MiB and released immediately after sanitizing argv; environment bytes can transiently enter memory. Arguments are never reconstructed from ps.
    ///
    /// `disk` has cumulative `read_bytes`, `written_bytes`, `window_read_bytes`, `window_written_bytes`, and its actual `window_seconds` (native readings can differ slightly from the CPU interval). Regressed counters give null window changes with a skipped entry. Energy is a cumulative counter, not watts or an energy-impact score.
    ///
    /// `open_files` items have `fd`, `kind` and `path`; excludes cwd and sockets, includes executable/mapped files and descriptors. `sockets` items have `fd`, `kind`, `protocol`, `local`, `remote_ip`, `remote_port`, `remote_name`, and `state`. IPv4/IPv6 remote addresses and ports remain numeric; local and Unix socket endpoints retain lsof's text. Listening/unconnected/Unix sockets have null remote fields. Reverse lookups use one shared deadline of {const:LOOKUP_SECONDS} seconds for all distinct remote IPs; a failed or timed-out name stays the IP. Lookups may send DNS requests. Empty arrays mean observed empty lists, never denied reads.
    ///
    /// `file_activity` is root-only fs_usage read/write events over the requested window: items have `operation`, `path`, and `bytes` (requested byte count, null if absent). Paths can contain spaces; the counters in disk remain authoritative byte accounting. Paths absent from an event are resolved from an initial lsof descriptor reading and subsequent open/close events; duplication clears uncertain associations. Unresolved paths are null with a skipped entry. This is a best-effort trace of a changing process, not a durable file audit. fs_usage can lose events and reports this as skipped, never a complete empty list. Other filesystem operations are excluded.
    ///
    /// Optional denied, missing or malformed readings are null with `skipped` entries (`measurement`, `reason`, `fix`). Non-root callers observing another user's process receive only ps identity, resident and CPU; all other fields are skipped with `sudo hostvitals process PID --seconds N`. fs_usage is skipped unless already root. hostvitals never runs sudo. Protected processes may remain unreadable as root. Missing tools name a PATH repair; unsupported reports/private interfaces name an update. CLI reports are bounded to 32 MiB and 30 seconds; fs_usage is bounded to its window plus five seconds. Explicit fixture reads never fall back to the host.
    #[command(next_help_heading = "Run by the agent", verbatim_doc_comment)]
    Process {
        /// Positive process ID to observe.
        #[arg(value_parser = clap::value_parser!(u32).range(1..=2147483647))]
        pid: u32,
        /// Minimum observation window in whole seconds.
        #[arg(long, default_value_t = crate::process::DEFAULT_SECONDS, value_parser = clap::value_parser!(u64).range(1..))]
        seconds: u64,
    },
    /// Judges and measures CPU, memory, disk and network over one window for the host, every user's processes and their Apps.
    ///
    /// Run `hostvitals now --seconds 3 --top 10`. Reads two all-user `ps` and `nettop` snapshots, two structured `ioreg` block-storage inventories, cumulative host CPU ticks, memory sysctls, process responsibility, two accessible rusage readings, and `df -a -l -k -P` for every mounted local volume. Only reads; saves nothing. `--seconds` and `--top` require a whole number at least 1. Refusals name the corrected command. Missing or malformed process or host snapshots refuse with a retry line; optional native, disk, volume and network reads are null with a `skipped` entry.
    ///
    /// Answers `findings` before the raw sections. Other keys are `source` (`host` or `fixtures`), `window_seconds` (actual monotonic time between process readings), `host`, `processes` (the entire final snapshot), `apps` (totals of those processes), `volumes`, `rankings` and `skipped`. Host CPU covers the same window with minor snapshot collection skew. CPU percentages for processes and Apps use one logical core as 100%, so multithreaded work can exceed 100%. Host percentages cover all cores and total 100%.
    ///
    /// `findings` is [] when none of the fixed rules match. Each item has a stable `rule`, `subject`, `measured`, `threshold`, and `next_command` for the caller to run; now never executes that suggestion. A subject has `kind`: host, process or volume. Process subjects also retain `pid`, `started_at` and `name` from the final snapshot. The rules judge every Process before --top limits Rankings; App sums do not create extra findings for these rules. Missing CPU baselines and zero denominators produce no corresponding finding, rather than measured zeroes. Ratios are judged with exact integer comparisons before any floating-point display conversion.
    ///
    /// Rules: `memory_pressure` means pressure at {const:FINDING_PRESSURE} or worse, ordered normal < warn < critical; measured has `pressure`, threshold has `comparison`: >= and `pressure`. `swap_used` means swap used > {const:FINDING_SWAP_PERCENT}% of total swap; measured has `used_bytes`, `total_bytes` and `percent`, threshold has `comparison`: > and `percent`. Both have a host subject and suggest `hostvitals now --top 20` to inspect the memory Rankings again.
    ///
    /// `process_resident` means Resident > {const:FINDING_RESIDENT_PERCENT}% of host memory; measured has `resident_bytes`, `host_memory_bytes` and `percent`, threshold has `comparison`: > and `percent`. It suggests `hostvitals memory PID`. Byte percentages derive from the same raw values in host.memory and processes. `process_cpu` means CPU >= {const:FINDING_CPU_CORES} core across the actual Window; measured has `cpu_hundredths` (cumulative CPU change in hundredths of a second), `window_nanoseconds` (exact integer elapsed time), `window_seconds` (that same duration in seconds), and `cpu_percent` (the same value as the Process record). Its threshold has `comparison`: >= and `cores`; it suggests `hostvitals profile PID`. CPU uses the same matching PID/start-time baseline and monotonic counter change as processes, never lifetime CPU or a rounded percentage. Process suggestions use the observed PID; a Process can exit or that PID can be reused before the caller runs them.
    ///
    /// `host.cpu` has `user_percent` (including nice), `system_percent`, `idle_percent`, `logical_cores` and `physical_cores`. `host.load_average` is the 1, 5 and 15 minute load. `host.memory` has `total_bytes`, `free_percent` (kern.memorystatus_level, not a resident-memory subtraction), `pressure` (normal, warn or critical), `swap_used_bytes`, `swap_total_bytes`, and `compressed_bytes` (physical compressed storage). Memory and load are read at the end of the window.
    ///
    /// `host.network` has `window_seconds` (actual monotonic time between the two nettop readings), `bytes_in_per_second` (received), and `bytes_out_per_second` (sent). All network rates divide cumulative-byte changes by this interval; requested --seconds is never used as the divisor. Collection causes minor skew relative to the CPU window. Host network sums nettop's per-process accounting for TCP and UDP on all interfaces, including loopback; proxy or forwarded traffic may be accounted to multiple processes. It is not a physical-link speed. No address or far-end names are requested: nettop runs with -n and process summaries only, so now performs no DNS lookup and sends no network requests. No connection details or remote names are returned.
    ///
    /// `host.disk` has `read_bytes_per_second`, `written_bytes_per_second`, and `window_seconds`: its own actual monotonic interval between two `ioreg -a -r -c IOBlockStorageDriver -d 1` readings. Each unique registry identity contributes once. Exact unsigned cumulative counters are subtracted by identity before checked summation and division by this interval, never requested sleep time. This is block-device accounting including virtual devices: image and backing-device activity may both appear. Descendant APFS/container/volume Statistics are excluded. It does not measure exclusively physical-media traffic, and need not reconcile with Process disk accounting.
    ///
    /// A changed, duplicate, missing, empty or unrecognized registry inventory, truncated report, or failed disk source leaves both host directions null. Missing, invalid, regressed counters or checked sum overflow confined to one direction leave that direction null, retaining the other. A measured zero delta remains zero. host.disk.window_seconds is null on a failed source read; an observed interval remains available when identities changed or counters were unusable. Each unavailable host direction or whole report is counted in skipped with a retry/diagnostic command. Reads are bounded to {const:MEASUREMENT_LIMIT_MIB} MiB and {const:MEASUREMENT_TIMEOUT_SECONDS} seconds. No persistent baselines or resampling hide failures.
    ///
    /// Every Process has `disk` with `read_bytes` and `written_bytes` measured within this invocation's Window, rather than lifetime counters. Its `disk.window_seconds` is the actual monotonic interval between that Process's two accessible rusage readings, which may differ from CPU, host disk, network and other Processes' intervals due to collection skew. PID, start time and owner must match. A new/reused Process, failed reading or regressed counter is null for the affected direction and counted in skipped; an unknown baseline is never zero. Without root, other users' disk values are null with `sudo hostvitals now` for the caller. The final rusage reading also supplies current Footprint and lifetime peak; a failed initial disk reading does not hide a readable final Footprint.
    ///
    /// Each App's `disk` has `read_bytes` and `written_bytes`: checked sums of its responsible Processes' Window bytes. Missing inputs propagate independently by direction; overflow also gives null. These are sums of the contributors' reported intervals, not one shared rate interval. `processes_by_disk_written` and `apps_by_disk_written` in rankings contain up to --top full records descending by exact Window written_bytes, tied by ascending PID or app_pid. Null values are excluded and measured zeroes remain eligible.
    ///
    /// `volumes` lists every mounted local volume reported by df, each with `filesystem` and `mount_point` as its mount identity, `size_bytes`, `free_bytes` (available to the caller), and `free_share` (free_bytes / size_bytes, fraction 0..1). df's 1024-byte blocks are converted with checked arithmetic; no rounded Capacity percentage is used. Mount paths retain spaces. APFS volumes may share capacity and free space; these observations are never summed into a host capacity. Unknown/invalid/overflowed values are null; zero capacity has null free_share. A failed or unrecognized inventory gives volumes null, rather than an invented empty list. Skipped reports unavailable counts and a df diagnostic line.
    ///
    /// `volume_free` means free < {const:FINDING_VOLUME_FREE_PERCENT}% of size, compared exactly from the returned raw bytes. Exactly at the threshold, zero capacity and unknown capacity produce no Finding. Its subject has kind volume, filesystem and mount_point; measured has size_bytes, free_bytes and free_share. Threshold has comparison < and percent. Its next_command is exactly `diskreap scan`: the caller can investigate what holds space there. now never executes that suggestion or analyzes space ownership.
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
    /// `skipped` entries have `measurement`, `count` (affected Processes, Apps, volumes or host fields), `reason`, and `fix`. Without root, other users' footprints are null and get one combined entry with `sudo hostvitals now`; their CPU and resident memory remain readable and ranked. Own-process rusage failures are separately aggregated. A missing private responsibility interface needs an updated hostvitals, rather than invented App attribution. Explicit HOSTVITALS_PROBE_FIXTURES native fixtures are labelled source fixtures and warned on stderr; missing fixtures never fall back to live reads.
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
        /// A command name, `all` for the full manual, or omit for the index.
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
        .constant("PROFILE_DEFAULT_SECONDS", crate::profile::DEFAULT_SECONDS)
        .constant("PROFILE_DEFAULT_TOP", crate::profile::DEFAULT_TOP)
        .constant("PROFILE_GRACE_SECONDS", crate::tools::sample::GRACE_SECONDS)
        .constant("PROCESS_DEFAULT_SECONDS", crate::process::DEFAULT_SECONDS)
        .constant("ARGUMENT_LIMIT_MIB", crate::direct::arguments::LIMIT_MIB)
        .constant("LOOKUP_SECONDS", crate::tools::names::DEADLINE_SECONDS)
        .constant(
            "SECRET_FLAG_WORDS",
            hostvitals_policy::redaction::SECRET_FLAG_WORDS.join(", "),
        )
        .constant("PROBE_TIMEOUT_SECONDS", crate::tools::PROBE_TIMEOUT_SECONDS)
        .constant(
            "MEASUREMENT_TIMEOUT_SECONDS",
            crate::tools::MEASUREMENT_TIMEOUT_SECONDS,
        )
        .constant("MEASUREMENT_LIMIT_MIB", crate::tools::MEASUREMENT_LIMIT_MIB)
        .constant("MEMORY_DEFAULT_TOP", crate::memory::DEFAULT_TOP)
        .constant(
            "FINDING_VOLUME_FREE_PERCENT",
            hostvitals_policy::disk::VOLUME_FREE_PERCENT,
        )
        .constant(
            "FINDING_PRESSURE",
            hostvitals_policy::findings::memory_pressure_threshold().name(),
        )
        .constant(
            "FINDING_SWAP_PERCENT",
            hostvitals_policy::findings::SWAP_USED_PERCENT,
        )
        .constant(
            "FINDING_RESIDENT_PERCENT",
            hostvitals_policy::findings::PROCESS_RESIDENT_PERCENT,
        )
        .constant(
            "FINDING_CPU_CORES",
            hostvitals_policy::findings::PROCESS_CPU_CORES,
        )
}
#[cfg(test)]
mod tests {
    #[test]
    fn manual_is_whole() {
        agent_help::check(&super::manual()).unwrap();
    }
}
