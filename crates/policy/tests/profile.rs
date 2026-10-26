use hostvitals_policy::profile::summarize;

#[test]
fn busy_report_ranks_exclusive_paths_and_combines_top_frames_across_threads() {
    let summary = summarize(include_str!("fixtures/sample-busy.txt"), 2).unwrap();
    assert_eq!(summary.total_samples, 200);
    assert_eq!(summary.waiting_samples, 0);
    assert!(!summary.mostly_waiting);
    assert!(!summary.partial);
    assert_eq!(summary.hot_paths.len(), 2);
    assert_eq!(summary.hot_paths[0].sample_count, 100);
    assert_eq!(summary.hot_paths[0].share, 0.5);
    assert_eq!(
        summary.hot_paths[0].frames,
        [
            "start (in libsystem_pthread.dylib)",
            "worker (in worker)",
            "crunch(int) (in worker)"
        ]
    );
    assert_eq!(summary.hot_paths[1].sample_count, 80);
    assert_eq!(summary.hot_paths[1].share, 0.4);
    assert_eq!(summary.top_frames[0].frame, "crunch(int) (in worker)");
    assert_eq!(summary.top_frames[0].sample_count, 180);
    assert_eq!(summary.top_frames[1].sample_count, 20);
}

#[test]
fn mostly_waiting_is_explicit_and_waits_do_not_masquerade_as_hot_paths() {
    let summary = summarize(include_str!("fixtures/sample-waiting.txt"), 10).unwrap();
    assert_eq!(summary.total_samples, 100);
    assert_eq!(summary.waiting_samples, 90);
    assert!(summary.mostly_waiting);
    assert_eq!(summary.hot_paths.len(), 1);
    assert_eq!(summary.hot_paths[0].sample_count, 10);
    assert_eq!(summary.hot_paths[0].share, 0.1);
    assert_eq!(
        summary.top_frames[0].frame,
        "mach_msg2_trap (in libsystem_kernel.dylib)"
    );
    assert_eq!(summary.top_frames[0].sample_count, 90);
}

#[test]
fn ended_process_keeps_observed_stacks_and_missing_footer_is_partial() {
    let report = include_str!("fixtures/sample-ended.txt");
    let summary = summarize(report, 10).unwrap();
    assert_eq!(summary.total_samples, 26);
    assert_eq!(summary.waiting_samples, 26);
    assert!(summary.mostly_waiting);
    assert!(summary.hot_paths.is_empty());
    let truncated = report.split("Total number in stack").next().unwrap();
    let partial = summarize(truncated, 10).unwrap();
    assert!(partial.partial);
    assert_eq!(partial.total_samples, 26);
    assert_eq!(partial.top_frames[0].sample_count, 26);
}

#[test]
fn self_samples_small_leaves_and_recursion_are_not_double_counted() {
    let summary = summarize("Call graph:\n    10 Thread_1\n      10 recurse  (in worker) + 1\n        7 recurse  (in worker) + 2\n          4 tiny  (in worker) + 3\nBinary Images:\n", 10).unwrap();
    assert_eq!(summary.total_samples, 10);
    assert_eq!(
        summary
            .hot_paths
            .iter()
            .map(|p| p.sample_count)
            .collect::<Vec<_>>(),
        [4, 3, 3]
    );
    assert_eq!(summary.top_frames[0].frame, "recurse (in worker)");
    assert_eq!(summary.top_frames[0].sample_count, 6);
    assert_eq!(summary.top_frames[1].sample_count, 4);
    for bad in [
        "",
        "permission denied",
        "Call graph:\n",
        "Call graph:\n    2 start\n      3 child\n",
        "Call graph:\n    18446744073709551616 start\n",
    ] {
        assert!(summarize(bad, 10).is_err(), "{bad}");
    }
}

#[test]
fn waiting_requires_a_kernel_wait_symbol_and_a_strict_majority() {
    let report = "Call graph:\n    10 Thread_1\n      10 root  (in worker) + 1\n        5 mach_msg2_trap  (in libsystem_kernel.dylib) + 2\n        3 mach_msg2_trap  (in worker) + 3\n        2 read  (in libsystem_kernel.dylib) + 4\nBinary Images:\n";
    let summary = summarize(report, 10).unwrap();
    assert_eq!(summary.total_samples, 10);
    assert_eq!(summary.waiting_samples, 5);
    assert!(!summary.mostly_waiting);
    assert_eq!(summary.hot_paths.len(), 2);
    assert_eq!(
        summary.hot_paths[0].frames.last().unwrap(),
        "mach_msg2_trap (in worker)"
    );
    assert_eq!(summary.hot_paths[0].share, 0.3);
    assert_eq!(summary.hot_paths[1].share, 0.2);
    assert_eq!(summary.top_frames[0].sample_count, 5);
}

#[test]
fn thread_named_functions_remain_frames_and_blank_frames_are_rejected() {
    let summary = summarize("Call graph:\n    10 Thread_1\n      10 Thread_worker  (in worker) + 1\n        10 crunch  (in worker) + 2\nBinary Images:\n", 10).unwrap();
    assert_eq!(
        summary.hot_paths[0].frames,
        ["Thread_worker (in worker)", "crunch (in worker)"]
    );
    assert_eq!(
        summarize("Call graph:\n    10 \nBinary Images:\n", 10).unwrap_err(),
        "missing sample frame"
    );
}

#[test]
fn numeric_thread_named_symbols_are_distinct_from_thread_headers() {
    let summary = summarize("Call graph:\n    10 Thread_1\n      10 Thread_123  (in worker) + 1\n        10 crunch  (in worker) + 2\nBinary Images:\n", 10).unwrap();
    assert_eq!(
        summary.hot_paths[0].frames,
        ["Thread_123 (in worker)", "crunch (in worker)"]
    );
    assert_eq!(summary.total_samples, 10);
}
