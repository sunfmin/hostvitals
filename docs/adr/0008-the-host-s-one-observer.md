# Be the host's one observer, which its users call where it is installed

hostvitals answers what a Mac is doing now (CPU, memory, disk, network and energy, by Process and App, with Findings) and looks into one Process, for agents. It is the one observer of a host: a program that needs the host's state runs `hostvitals now` and reads its JSON rather than measuring the host itself. hostvitals depends on none of its users and knows nothing of them. Its output is their contract: what a user reads is what `hostvitals now --help` documents, its Findings and then its Process and App Rankings, and a measurement it could not take stays null or Skipped with its reason, never zero.

The cause was 2026-10-25's memory pressure: `fseventsd`, a root Process running 13 days at 9.9 GiB, outside every fleet. afkconvoy's own "hottest processes outside the workers" gave numbers with no judgement, and whats-hot's scripts each measured a part. One observer gives every user the same Findings for the same host.

Each user's side:

- **afkconvoy** uses `hostvitals now` for the "hottest processes outside the workers" part of a load or memory host evidence when `hostvitals` is on `PATH`, and its own measurement otherwise (Linux, or not installed), naming which source it used. Its formula and manifests do not depend on hostvitals. Each worker's process tree and the fleet's ten-second hot sample stay afkconvoy's. Built in [sunfmin/control#61](https://github.com/sunfmin/control/issues/61); afkconvoy has no ADR of its own for it.
- **Guild**: guildd reads its machine only through hostvitals, and those readings are what its report carries ([Guild ADR 0004](https://github.com/sunfmin/guild/blob/main/docs/adr/0004-machines-talk-through-guildds-instance-interface-over-tailscale.md)); `guild install` installs hostvitals where it is missing ([sunfmin/control#818](https://github.com/sunfmin/control/issues/818)).
- **whats-hot** is a pointer to hostvitals ([whats-hot ADR 0005](https://github.com/sunfmin/whats-hot/blob/main/skills/whats-hot/docs/adr/0005-superseded-by-hostvitals.md)); what it carried over is ADR-0004.
- **diskreap** keeps "what holds disk space". A volume with less than 10% free gives a `volume_free` Finding whose next command is `diskreap scan`; hostvitals never runs it (ADR-0004).

Sources: this decision was sunfmin/control's ADR 0004 and moved here in [sunfmin/control#769](https://github.com/sunfmin/control/issues/769), by Felix's words of 2026-11-12: 「跨项目的全部放到他们项目里面的 ADR 里面去」; its tickets were sunfmin/control#50–#61.

## Considered Options

- **afkconvoy's formula depends on hostvitals and drops its own measurement.** One path, but afkconvoy ships for five platforms and hostvitals only for macOS (ADR-0001): installing afkconvoy through Homebrew on Linux would fail. Rejected.
- **afkconvoy's ten-second hot sample also calls hostvitals.** An external CLI every ten seconds per fleet costs too much, and the hot sample looks only at the fleet's own workers, so it needs none of hostvitals' whole-host view. Rejected.

## Consequences

- afkconvoy has two sources for the Processes outside its workers, on purpose: hostvitals' on a Mac where it is installed, its own elsewhere.
