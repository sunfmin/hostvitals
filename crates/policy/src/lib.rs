//! Decisions about whether an observation may run, with no access to the host.
extern crate flux_alloc;
extern crate flux_core;

use flux_rs::attrs::*;
pub mod apps;
pub mod files;
pub mod network;
pub mod redaction;

/// A probe may execute only after its executable is found and its privilege requirement is met.
#[spec(fn(present: bool, needs_root: bool, is_root: bool) -> bool[present && (!needs_root || is_root)])]
pub fn may_probe(present: bool, needs_root: bool, is_root: bool) -> bool {
    present && (!needs_root || is_root)
}

/// Only matching process identities with monotonic cumulative counters yield a delta.
#[spec(fn(before: u64, after: u64, same_process: bool) -> Option<u64[after - before]>[same_process && after >= before])]
pub fn counter_delta(before: u64, after: u64, same_process: bool) -> Option<u64> {
    if same_process && after >= before {
        Some(after - before)
    } else {
        None
    }
}

/// Root can request every footprint; an ordinary observer can request only its own user's.
#[spec(fn(observer: u32, owner: u32) -> bool[observer == 0 || observer == owner])]
pub fn may_read_footprint(observer: u32, owner: u32) -> bool {
    observer == 0 || observer == owner
}

/// CPU time is in ps hundredths of a second; one core fully occupied is 100 percent.
pub fn cpu_percent(hundredths: u64, seconds: f64) -> f64 {
    hundredths as f64 / seconds
}

/// Unsigned host tick counters wrap at 32 bits; widen their delta before adding cores' modes.
#[spec(fn(before: u32, after: u32) -> u64[if after >= before {after - before} else {4294967296 + after - before}])]
pub fn tick_delta(before: u32, after: u32) -> u64 {
    if after >= before {
        (after - before) as u64
    } else {
        (u32::MAX - before) as u64 + after as u64 + 1
    }
}

/// A partial set of measurements never becomes an apparently complete total.
pub fn add_bytes(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => a.checked_add(b),
        _ => None,
    }
}
pub fn add_rates(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a + b),
        _ => None,
    }
}

/// Rankings descend by the resource and break ties with ascending process identity.
pub fn rank_order(a: f64, a_pid: u32, b: f64, b_pid: u32) -> std::cmp::Ordering {
    if a > b {
        std::cmp::Ordering::Less
    } else if a < b {
        std::cmp::Ordering::Greater
    } else {
        a_pid.cmp(&b_pid)
    }
}

#[cfg(flux)]
#[allow(dead_code)]
mod rejected {
    #[flux_rs::should_fail]
    fn reused_pid_cpu() {
        flux_rs::assert(super::counter_delta(100, 200, false).is_some());
    }
    #[flux_rs::should_fail]
    fn another_users_footprint() {
        flux_rs::assert(super::may_read_footprint(501, 0));
    }
    #[flux_rs::should_fail]
    fn counter_change_is_not_zero() {
        flux_rs::assert(super::counter_delta(100, 150, true).unwrap() == 0);
    }
    #[flux_rs::should_fail]
    fn wrapped_ticks_are_not_zero() {
        flux_rs::assert(super::tick_delta(u32::MAX, 0) == 0);
    }
    // This would permit a root-only measurement before obtaining root.
    #[flux_rs::should_fail]
    fn unprivileged_root_probe() {
        flux_rs::assert(super::may_probe(true, true, false));
    }
    // This would claim that finding a command can be skipped.
    #[flux_rs::should_fail]
    fn absent_executable_probe() {
        flux_rs::assert(super::may_probe(false, false, true));
    }
}

pub mod memory;
pub mod profile;
