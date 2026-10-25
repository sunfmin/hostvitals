# Keep windowed CPU and incomplete App totals explicit

`now` divides the change in `ps` cumulative CPU time by actual monotonic time between its two process readings, with one logical core equal to 100%; host CPU uses Darwin's aggregate tick deltas, including nice in user time. Matching PID and start time is required: new or replaced Processes and regressed counters have null CPU, since treating an unobserved baseline as zero would invent a Window measurement.

Rank memory by Resident for both Processes and Apps because it is observable for every user without root. App totals follow macOS responsibility and propagate missing measurements as null, so partial Footprints cannot appear comparable to complete ones; the sum of lifetime process peaks is explicitly not a simultaneous App peak. A missing responsibility read leaves attribution null rather than replacing the domain's App with a parent or a same-name grouping.
