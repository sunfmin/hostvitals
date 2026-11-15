# Coordinate the live ended-Process observation

The live Profile regression used to launch `/bin/sleep 0.5`, then launch
hostvitals and ask for a one-second Window. That assumes observer startup and
sample's initialization leave time to collect stacks. Neither is synchronized
with the child's lifetime.

During control#125, the unchanged test passed in two bounded attempts. The
second report was captured before cleanup: 374 samples, 373 waiting, with a
complete counted graph accepted by the existing policy parser. No further
unchanged reproduction loop was run.

Two controlled startup delays tested the lifetime assumption directly. A
600 ms delay before launching hostvitals left a zero-byte retained report and
sample's exit-255 diagnostic. A 450 ms delay also left a zero-byte retained
report, this time with a successful sample exit and no call graph. Both made
the existing positive assertion fail. These are collection failures rather
than useful partial graphs rejected by the parser. The exact historical
`no sampled call stacks found` report was not recovered; its test had already
removed it. We do not infer its contents from that diagnostic.

The repaired test uses a sleeping target in the existing `probe-cli` example.
The target announces readiness and stays alive until released, with a bounded
40-second lifetime, longer than the test's waits. The test starts the real hostvitals binary through a
wrapper that delays sampling by 600 ms, exceeding the old child's lifetime,
and then invokes the real `/usr/bin/sample` with hostvitals' unchanged arguments.
The wrapper uses `script -qFe` to expose sample's collection announcement
through a flushed pseudo-terminal. Redirecting progress directly to a file
buffered the announcement until the observation was over, which is unsuitable
for coordination.

After the announcement, the test waits until sample has taken stacks, then
releases the target and checks that it exits before the requested 20-second
Window completes. sample stops once the target ends, so the long Window costs
nothing when the host is quiet. The announcement precedes the first stack by
as long as the host makes sample wait: a fixed two-second allowance failed
the gate at load average about 160 (control#1134), with a retained report
that had its header and Binary Images and an empty call graph. Stopping
sample for three seconds after its announcement reproduced that skipped
answer under the fixed allowance in one of three runs.

The test learns that stacks were taken from the target's suspend count,
which the test reads through the target's Mach task name port. That port
needs no task port and is readable by the target's owner. sample suspends the
target while it takes each stack. Its first four completed suspensions take
none: a target killed after k of them left k - 4 stacks (k = 5, 10 and 50 gave
1, 6 and 46; k = 1 and 2 gave none). The test therefore waits for 50 completed
suspensions, polled without sleeping. A missed resume only undercounts, so
the wait errs long. No suspension preceded the announcement. A deadline of
15 seconds after the announcement fails naming how many of the 50 it saw.

Readiness, collection, stacks and completion each have deadlines; owned
children are killed and reaped on assertion failures. The observation itself
also has the production timeout. There is one invocation, with no retry until
success. Under 400 busy loops (load average 240 to 551), ten runs passed; the
50th suspension came 1.2 to 3.0 seconds after the announcement, and the
reports held 793 to 1881 samples. With sample stopped for three seconds,
three runs passed with 78 to 93 samples.

The test still requires Available output, positive sample and waiting counts,
and Mostly waiting. It compares every returned hot path and top frame with a
summary of the retained report, and checks report mode 0600 and containing
directory mode 0700. It prints the report before cleanup so a future failure
keeps its evidence. The real report remains the sole observation used by the
product; the wrapper's progress is only a test coordination signal.

Policy parsing, source selection, privileges and the RETAIN ruling are
unchanged. Empty files and counted graphs without samples remain Skipped,
retain their exact bytes and have no invented counts or rankings. They now
recommend retrying while the Process runs. A real collector failure still
takes precedence, including its existing denial diagnostic and retry line.
