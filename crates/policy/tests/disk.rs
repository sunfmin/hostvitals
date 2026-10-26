use hostvitals_policy::disk::{Counters, host_change, process_change};
use std::collections::BTreeMap;

fn counters(read: Option<u64>, written: Option<u64>) -> Counters {
    Counters {
        read_bytes: read,
        written_bytes: written,
    }
}

#[test]
fn host_subtracts_exact_counters_by_identity_before_summing() {
    let before = BTreeMap::from([
        (2, counters(Some(u64::MAX - 5), Some(100))),
        (1, counters(Some(100), Some(200))),
    ]);
    let after = BTreeMap::from([
        (1, counters(Some(400), Some(600))),
        (2, counters(Some(u64::MAX), Some(100))),
    ]);
    let change = host_change(&before, &after).unwrap();
    assert_eq!(change.read_bytes, Some(305));
    assert_eq!(change.written_bytes, Some(400));
}

#[test]
fn low_free_space_uses_exact_bytes_with_no_zero_or_unknown_ratio() {
    use hostvitals_policy::disk::low_free;
    for (size, free, expected) in [
        (Some(100), Some(9), true),
        (Some(100), Some(10), false),
        (Some(100), Some(11), false),
        (Some(0), Some(0), false),
        (None, Some(0), false),
        (Some(100), None, false),
        (Some(u64::MAX), Some(u64::MAX / 10), true),
        (Some(u64::MAX), Some(u64::MAX / 10 + 1), false),
    ] {
        assert_eq!(low_free(size, free), expected, "{size:?}/{free:?}");
    }
}

#[test]
fn inventories_must_match_and_each_direction_keeps_its_own_completeness() {
    let old = BTreeMap::from([(1, counters(Some(100), Some(200)))]);
    for new in [
        BTreeMap::new(),
        BTreeMap::from([(2, counters(Some(110), Some(220)))]),
        BTreeMap::from([
            (1, counters(Some(110), Some(220))),
            (2, counters(Some(0), Some(0))),
        ]),
    ] {
        assert!(host_change(&old, &new).is_err());
        assert!(host_change(&new, &old).is_err());
    }
    for read in [None, Some(99)] {
        let new = BTreeMap::from([(1, counters(read, Some(220)))]);
        let change = host_change(&old, &new).unwrap();
        assert_eq!(change.read_bytes, None);
        assert_eq!(change.written_bytes, Some(20));
    }
    let old = BTreeMap::from([
        (1, counters(Some(0), Some(0))),
        (2, counters(Some(0), Some(0))),
    ]);
    let new = BTreeMap::from([
        (1, counters(Some(u64::MAX), Some(0))),
        (2, counters(Some(1), Some(0))),
    ]);
    let change = host_change(&old, &new).unwrap();
    assert_eq!(change.read_bytes, None);
    assert_eq!(change.written_bytes, Some(0));
}

#[test]
fn process_requires_two_matching_readings_and_monotonic_directions() {
    let old = counters(Some(100), Some(200));
    let new = counters(Some(99), Some(220));
    for (before, after, same) in [
        (None, Some(&new), true),
        (Some(&old), None, true),
        (Some(&old), Some(&new), false),
    ] {
        let change = process_change(before, after, same);
        assert_eq!(change.read_bytes, None);
        assert_eq!(change.written_bytes, None);
    }
    let change = process_change(Some(&old), Some(&new), true);
    assert_eq!(change.read_bytes, None);
    assert_eq!(change.written_bytes, Some(20));
}

#[test]
fn write_rankings_preserve_exact_integers_zeroes_identity_ties_and_top() {
    use hostvitals_policy::disk::rank_written;
    let values = [
        (10, None),
        (9, Some(0)),
        (8, Some(9007199254740992)),
        (7, Some(9007199254740993)),
        (6, Some(9007199254740992)),
    ];
    assert_eq!(rank_written(&values, 10), vec![7, 6, 8, 9]);
    assert_eq!(rank_written(&values, 2), vec![7, 6]);
    assert!(rank_written(&values, 0).is_empty());
}

#[test]
fn app_disk_totals_propagate_unknown_and_checked_overflow_by_direction() {
    use hostvitals_policy::apps::{ProcessUsage, summarize};
    let usage = |app, read, written| ProcessUsage {
        energy_impact: Some(0.0),
        app_pid: app,
        cpu_percent: Some(0.0),
        resident_bytes: 0,
        footprint_bytes: Some(0),
        peak_footprint_bytes: Some(0),
        bytes_in_per_second: Some(0.0),
        bytes_out_per_second: Some(0.0),
        disk_read_bytes: read,
        disk_written_bytes: written,
    };
    let apps = summarize(&[
        usage(Some(1), Some(u64::MAX), Some(10)),
        usage(Some(1), Some(1), Some(20)),
        usage(Some(2), None, Some(30)),
        usage(Some(2), Some(1), Some(40)),
        usage(None, Some(1), Some(1)),
    ]);
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0].disk_read_bytes, None);
    assert_eq!(apps[0].disk_written_bytes, Some(30));
    assert_eq!(apps[1].disk_read_bytes, None);
    assert_eq!(apps[1].disk_written_bytes, Some(70));
}
