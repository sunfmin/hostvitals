//! Windowed block-device and Process accounting; these are independent observations.
use flux_rs::attrs::*;
use std::collections::BTreeMap;

pub const VOLUME_FREE_PERCENT: u64 = 10;

/// Judges raw bytes before rounding; unknown and zero capacity cannot produce a ratio Finding.
pub fn low_free(size: Option<u64>, free: Option<u64>) -> bool {
    match (size, free) {
        (Some(size), Some(free)) => below_free_threshold(size, free),
        _ => false,
    }
}

#[spec(fn(size: u64, free: u64) -> bool[size > 0 && free * 100 < size * VOLUME_FREE_PERCENT])]
fn below_free_threshold(size: u64, free: u64) -> bool {
    size > 0 && free as u128 * 100 < size as u128 * VOLUME_FREE_PERCENT as u128
}

pub fn free_share(size: Option<u64>, free: Option<u64>) -> Option<f64> {
    match (size, free) {
        (Some(size), Some(free)) if size > 0 && free <= size => Some(free as f64 / size as f64),
        _ => None,
    }
}

#[cfg(flux)]
#[allow(dead_code)]
mod rejected {
    #[flux_rs::should_fail]
    fn exactly_ten_percent_is_low() {
        flux_rs::assert(super::below_free_threshold(100, 10));
    }
    #[flux_rs::should_fail]
    fn zero_capacity_is_low() {
        flux_rs::assert(super::below_free_threshold(0, 0));
    }
}

pub struct Counters {
    pub read_bytes: Option<u64>,
    pub written_bytes: Option<u64>,
}

pub struct Change {
    pub read_bytes: Option<u64>,
    pub written_bytes: Option<u64>,
}

/// Missing baselines and changed identities never yield lifetime bytes as Window activity.
pub fn process_change(before: Option<&Counters>, after: Option<&Counters>, same: bool) -> Change {
    match (before, after) {
        (Some(before), Some(after)) if same => Change {
            read_bytes: delta(before.read_bytes, after.read_bytes),
            written_bytes: delta(before.written_bytes, after.written_bytes),
        },
        _ => Change {
            read_bytes: None,
            written_bytes: None,
        },
    }
}

fn delta(before: Option<u64>, after: Option<u64>) -> Option<u64> {
    match (before, after) {
        (Some(before), Some(after)) => super::counter_delta(before, after, true),
        _ => None,
    }
}

/// Counts each registry identity once, and refuses to compare different or empty inventories.
/// A direction remains unknown if any member is missing, regressed or its checked sum overflows.
pub fn host_change(
    before: &BTreeMap<u64, Counters>,
    after: &BTreeMap<u64, Counters>,
) -> Result<Change, &'static str> {
    if before.is_empty() || after.is_empty() {
        return Err("empty block-storage inventory");
    }
    if before.keys().ne(after.keys()) {
        return Err("block-storage identities changed during the Window");
    }
    let mut total = Change {
        read_bytes: Some(0),
        written_bytes: Some(0),
    };
    for (id, new) in after {
        let change = process_change(before.get(id), Some(new), true);
        total.read_bytes = super::add_bytes(total.read_bytes, change.read_bytes);
        total.written_bytes = super::add_bytes(total.written_bytes, change.written_bytes);
    }
    Ok(total)
}

/// Exact byte Rankings, descending with deterministic PID ties; zeroes remain eligible.
pub fn rank_written(values: &[(u32, Option<u64>)], top: usize) -> Vec<u32> {
    let mut measured: Vec<_> = values
        .iter()
        .filter_map(|(pid, bytes)| bytes.map(|n| (*pid, n)))
        .collect();
    measured.sort_by(|(a_pid, a), (b_pid, b)| b.cmp(a).then(a_pid.cmp(b_pid)));
    measured.into_iter().take(top).map(|(pid, _)| pid).collect()
}
