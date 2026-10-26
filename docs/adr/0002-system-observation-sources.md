# Use Apple's entitled tools and narrowly scoped native reads

The source selection follows the measurements recorded in control#50 on macOS 27, 2026-10-25, without root. Direct third-party reads reach a user's own process counters but cannot replace Apple's entitled tools for other users' processes, call stacks, memory regions or verified network accounting; the source is chosen by what it can actually observe, an exception to agent-cli's rule to reach everything through a CLI.

| Observation | Source and reason |
| --- | --- |
| Process list, every Process's CPU and memory | `ps`; it is setuid root. A third-party binary's task information and rusage fail for root processes such as fseventsd, WindowServer and launchd, even though their names and pids are visible. `top` holds `com.apple.system-task-ports.read`. |
| Own Processes' cumulative CPU, resident, footprint, lifetime peak footprint, disk bytes and energy | `proc_pid_rusage` v6 adds counters to the `ps` observations; it is not the inventory's source. |
| Call stacks and memory regions | `sample`, `vmmap`, `footprint` hold task-port entitlements. Direct `task_for_pid` fails even on a binary's own child. |
| Per-process network bytes | `nettop`, holding `com.apple.private.network.statistics`, sees every user's Processes. A direct kernel statistics socket connected, but its data was not verified. |
| CPU, GPU and ANE Power | IOReport's private Energy Model channels; a non-root subscription sampled 278 channels in the recorded measurement. `powermetrics` requires root. Channel availability varies by Mac. |
| A Process's App | Private `responsibility_get_pid_responsible_for_pid`; no equivalent non-root CLI was found. A Process without another responsible Process is its own App. |
| Exact command arguments | `KERN_PROCARGS2`, parsed only through argc and redacted before leaving the native reader. The bounded buffer can transiently include environment bytes; the privacy contract is in the observation ADR. `ps` command text loses argument boundaries and is never used to reconstruct argv. |
| Cwd, open files and sockets | `lsof -nP` with NUL-delimited fields; numeric IPv4/IPv6 endpoints remain intact. Reverse names use `dscacheutil` under one shared two-second deadline, retaining the IP on failure. |
| Files read/written over a Window | Root-only `fs_usage -w -t SECONDS -f filesys PID`; read/write events retain paths when provided, requested bytes rather than cumulative disk accounting, and event-loss/collector diagnostics skip the report. |
| Root-process open files and sockets | Not available to a non-root direct reader or `lsof`; never silently claim otherwise. |

Each native source has one module in the effect crate, and private symbols are looked up at runtime so disappearance becomes a doctor finding rather than a loader failure. Doctor checks its own Process, not the readability of every other Process: it discovers Energy Model channels, subscribes and samples to distinguish enumeration from actual access. Native results can be replaced by explicit, labelled JSON fixtures; CLI stand-ins occupy PATH. A live smoke test checks fields and ranges rather than exact host-dependent measurements. The C layouts for rusage and task information follow Darwin's SDK headers; the IOReport C signatures were checked against [Stats's bridge](https://github.com/exelban/stats/blob/master/Modules/CPU/bridge.h).
