# Consecutive loops and the pinned Flux checker

[control#98](https://github.com/sunfmin/control/issues/98) is resolved by a
source-level remedy: move the final stack-draining loop into `finish_graph`.
The complete supplied parser, including that helper, passes the existing
checker. The Flux revision, spec crates, nightly, solver versions and proof
requirements stay pinned as before. The underlying checker defect described
below remains in that revision; this is a verified source shape for #57 to use.

## Reproduction and reduction

On 2026-10-26, the issue's exact source was added temporarily as a policy
module. `./verify.sh profile` exited 101:

```text
error: internal compiler error: crates/flux-refineck/src/checker.rs:2453:28: impossible case reached
  --> crates/policy/src/profile.rs:40:5
   | for line in graph.lines() { ... }
note: bug caught [def_id: ... profile::summarize ...]
```

The same error reproduces without strings, trees, allocation, fallible calls,
refinement annotations or parser behavior:

```rust
pub fn summarize(mut first: bool, mut second: bool) {
    while first {
        if second {
            break;
        }
        first = false;
    }
    while second {
        second = false;
    }
}
```

Each variant was checked as the temporary `profile` policy module with
`./verify.sh profile`:

| Source | Exit | Result |
| --- | --- | --- |
| Exact issue source | 101 | Marker crash in `summarize` |
| Stubbed row parser, ranking and stack operations | 101 | Same crash |
| Boolean example above | 101 | Same crash |
| Boolean example with the conditional break removed | 0 | Checked |
| Boolean example with the second loop removed | 0 | Checked |
| Complete issue source with the stack drain extracted | 0 | All ten reproducer functions/closures checked |

The committed regression fixture was also checked **before** the extraction:
`./verify.sh call_graph_regression` exited 101 with the same line-2453 crash
in `call_graph_regression::summarize`. The extraction made that command exit 0.
The temporary product module was then removed; the fixture is the only parser
source introduced by this issue.

## Why the marker is missing

This diagnosis uses the pinned Flux source, the pinned nightly's borrow-checking
MIR (`rustc -Zdump-mir=all`), and the checker trace/backtrace from
`FLUXFLAGS=-Fdump-checker-trace=trace RUST_BACKTRACE=1 ./verify.sh profile`.

In the boolean example's analysis MIR, the first loop's false condition and
its `break` meet at `bb7`. That block contains only `StorageDead` statements
and a `goto bb8`. `bb8` is the second loop's header and has `bb7` as its
immediate dominator. Flux lowers storage statements to no-ops.

At the pinned revision, in `crates/flux-refineck/src/checker.rs`:

1. `check_goto` (around line 1206) follows `is_dummy_join` directly to the
   successor. `bb7` qualifies: multiple predecessors, only no-ops, one goto,
   and no ghost statements. It is never checked as a basic block.
2. Markers are assigned at the end of `check_basic_block` (line 540). The
   skipped `bb7` therefore has no marker.
3. `ShapeMode::check_goto_join_point` (line 2271) asks for the second loop
   header's dominator marker when constructing its initial environment.
4. `marker_at_dominator` (lines 2442–2453) uses the original MIR's immediate
   dominator and unwraps `markers[bb7]`, which is `None`.

The backtrace reaches `marker_at_dominator` through
`ShapeMode::check_goto_join_point` and two successive `check_goto` calls.
The trace ends on the first loop's false-condition path, before checking the
second loop or revisiting a queued block. This rules out solver failure and
marker clearing during rechecking. The skip optimization and dominator lookup
disagree about which blocks have markers; this is a checker defect on valid
Rust, not a missing parser precondition.

## The checked remedy

The product parser at
[`crates/policy/src/profile.rs`](../../crates/policy/src/profile.rs)
adopts the verified remedy from #98's original fixture, with this structural change:

```rust
// At the end of summarize, after the graph-reading loop:
finish_graph(&mut stack, &mut paths)?;
rank(paths, top, !complete)

// Private helper in the same pure module:
fn finish_graph(
    stack: &mut Vec<Node>,
    paths: &mut BTreeMap<Vec<String>, u64>,
) -> Result<(), &'static str> {
    while !stack.is_empty() {
        finish_node(stack, paths)?;
    }
    Ok(())
}
```

The helper completes the same pending frames, mutates the same collections and
propagates the same errors before ranking. The graph-reading loop still stops
at each footer, skips blank lines, and preserves the missing-footer flag.
Each function now has its own checked control-flow graph.

The normal `./verify.sh` and gate verify the product parser's body and every
helper alongside the existing policy. No function is trusted or ignored.
`flux_call_graph` integration tests now call the actual public summarizer and
retain all of #98's assertions for three footer exits, EOF, pending-frame
counts, waiting samples, rankings, and an error raised while draining the stack.
The duplicate parser fixture and its cfg-only inclusion were removed when #57
adopted the remedy. Inlining the drain into `summarize` again reproduces the
checker crash and makes verification fail.

Run the focused runtime checks with
`cargo test -p hostvitals-policy --test flux_call_graph <test-name> -- --exact`.
Run the complete verification and regression suite through the normal gate,
`afkconvoy exclusive -- ./gate.sh`; its policy summary must contain zero trusted
and zero ignored functions. For focused checking of the actual parser, run
`./verify.sh profile`. #57 supplies the sampling command and report retention
around this pure implementation.
