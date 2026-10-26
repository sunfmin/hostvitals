// The supplied control#98 parser is a verification fixture, not a product interface.
#[path = "fixtures/call-graph.rs"]
mod call_graph;

use call_graph::summarize;

const GRAPH: &str = "Call graph:
    10 Thread_1
      10 root  (in worker) + 1
        6 work  (in worker) + 2
        4 mach_msg2_trap  (in libsystem_kernel.dylib) + 3
";

#[test]
fn loop_exit_and_stack_drain_preserve_counts_and_footer_detection() {
    for footer in [
        "Total number in stack",
        "Sort by top of stack",
        "Binary Images:",
    ] {
        // A footer must stop parsing: this row would otherwise be rejected.
        let report = format!("{GRAPH}\n{footer}\nnot a graph row\n");
        let summary = summarize(&report, 10).unwrap();
        assert!(!summary.partial);
        assert_eq!(summary.total_samples, 10);
        assert_eq!(summary.waiting_samples, 4);
        assert!(!summary.mostly_waiting);
        assert_eq!(summary.hot_paths.len(), 1);
        assert_eq!(
            summary.hot_paths[0].frames,
            ["root (in worker)", "work (in worker)"]
        );
        assert_eq!(summary.hot_paths[0].sample_count, 6);
        assert_eq!(summary.hot_paths[0].share, 0.6);
        assert_eq!(summary.top_frames.len(), 2);
        assert_eq!(summary.top_frames[0].frame, "work (in worker)");
        assert_eq!(summary.top_frames[0].sample_count, 6);
        assert_eq!(
            summary.top_frames[1].frame,
            "mach_msg2_trap (in libsystem_kernel.dylib)"
        );
        assert_eq!(summary.top_frames[1].sample_count, 4);
    }

    let partial = summarize(GRAPH, 1).unwrap();
    assert!(partial.partial);
    assert_eq!(partial.total_samples, 10);
    assert_eq!(partial.hot_paths[0].sample_count, 6);
    assert_eq!(partial.top_frames.len(), 1);
}

#[test]
fn stack_drain_still_propagates_inconsistent_counts() {
    for footer in ["", "Binary Images:\n"] {
        let report = format!("Call graph:\n    2 root\n      3 child\n{footer}");
        assert_eq!(
            summarize(&report, 10).unwrap_err(),
            "child sample counts exceed parent"
        );
    }
}
