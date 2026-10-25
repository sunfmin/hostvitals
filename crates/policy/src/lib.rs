//! Decisions about whether an observation may run, with no access to the host.
extern crate flux_alloc;
extern crate flux_core;

use flux_rs::attrs::*;

/// A probe may execute only after its executable is found and its privilege requirement is met.
#[spec(fn(present: bool, needs_root: bool, is_root: bool) -> bool[present && (!needs_root || is_root)])]
pub fn may_probe(present: bool, needs_root: bool, is_root: bool) -> bool {
    present && (!needs_root || is_root)
}

#[cfg(flux)]
#[allow(dead_code)]
mod rejected {
    // This would permit a root-only measurement before obtaining root.
    #[should_fail]
    fn unprivileged_root_probe() {
        flux_rs::assert(super::may_probe(true, true, false));
    }
    // This would claim that finding a command can be skipped.
    #[should_fail]
    fn absent_executable_probe() {
        flux_rs::assert(super::may_probe(false, false, true));
    }
}
