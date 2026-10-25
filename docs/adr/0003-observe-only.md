# Observe without controlling a Process

Every hostvitals command only observes: none signals, renices, kills, or attaches in order to change a Process. It never runs `sudo`, installs a dependency or prompts; a root-only measurement is Skipped with its reason and the line the caller can elect to run. Because every command already shows what it can observe and changes nothing, the commands themselves satisfy agent-cli's requirement for a command that says what would happen.

Every measurement command samples within one Window and exits, with no daemon, history or authoritative cache. Doctor's independent capability probes are an inventory, not a simultaneous resource measurement. “Since when” comes only from macOS's own counters: a Process's start time, cumulative CPU, lifetime peak Footprint and cumulative disk bytes. Observer overhead is unavoidable; report files, captured memory graphs and persistent tracing sessions are not created.
