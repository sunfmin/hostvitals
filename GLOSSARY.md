# Host observations

The language of observing a Mac and its processes without changing their behaviour.

## Language

**Process**: A running program identified by a process ID and a start time. A reused process ID denotes a different Process.
_Avoid_: Task (when meaning the running program).

**App**: The Process macOS holds responsible for a Process, as Activity Monitor groups them. A Process with no other responsible Process is its own App.
_Avoid_: Parent, bundle (neither necessarily identifies the responsible Process).

**Window**: The span over which a command measures rates. A cumulative counter describes the lifetime counted by macOS rather than a rate over a Window.
_Avoid_: History, session.

**Ranking**: Processes or Apps ordered by one resource, limited to the top N.
_Avoid_: Offenders (a high rank alone is not a fault).

**Finding**: A judgement by a fixed rule, including the rule, measured numbers, threshold and next command.
_Avoid_: Guess, anomaly (without its rule).

**Skipped**: A measurement not taken, with the reason and the command line that takes it.
_Avoid_: Zero, unavailable reading (neither means a measured zero).

**Resident**: Memory a Process currently holds in RAM. It differs from the amount of memory macOS charges to that Process.
_Avoid_: Footprint, allocated memory.

**Footprint**: Memory macOS charges to a Process, including its accounting for compressed and shared memory. It is distinct from Resident memory.
_Avoid_: Resident, RSS.

**Memory pressure**: macOS's judgement of memory availability: normal, warn or critical.
_Avoid_: Percent used (which is not the pressure judgement).

**Energy impact**: macOS's per-process energy score, rather than a physical power measurement.
_Avoid_: Watts, Power.

**Power**: The rate at which a hardware component draws energy, expressed in watts.
_Avoid_: Energy impact, energy (energy accumulates over time).

**Profile**: A sample of a Process's call stacks over a Window, used to explain where it spends time.
_Avoid_: Trace (when only sampled stacks were observed).

**Hot path**: A sequence of calls accounting for a substantial share of a Profile's samples.
_Avoid_: Root cause (a frequent path alone does not establish causation).

**Redacted**: An observed value deliberately withheld from output to avoid revealing sensitive information; its omission is explicit.
_Avoid_: Missing, unknown (neither states that a value was withheld).
