//! Process-memory reads and JSON presentation; interpretation lives in the pure policy crate.
use crate::tools::{footprint, vmmap};
use hostvitals_policy::memory::{self, Memory};
use serde_json::{Value, json};

pub(crate) const DEFAULT_TOP: u32 = 15;

pub(crate) fn answer(pid: u32, top: u32) -> Value {
    let command = format!("hostvitals memory {pid} --top {top}");
    let pid_arg = pid.to_string();
    let (region_kinds, malloc_zones) = match vmmap::summary(&pid_arg) {
        Ok(text) => match memory::parse_summary(&text) {
            Ok(summary) => {
                let kinds: Vec<_> = summary
                    .kinds
                    .into_iter()
                    .map(|r| json!({"kind":r.kind,"memory":sizes(r.memory),"regions":r.regions}))
                    .collect();
                let zones: Vec<_> = summary.zones.into_iter().map(|r| json!({"zone":r.zone,"memory":sizes(r.memory),"allocations":r.allocations,"allocated_bytes":r.allocated_bytes,"regions":r.regions})).collect();
                (available(kinds), available(zones))
            }
            Err(reason) => {
                let skipped = malformed(reason, &command);
                (skipped.clone(), skipped)
            }
        },
        Err(reason) => {
            let skipped = skipped(reason, &command);
            (skipped.clone(), skipped)
        }
    };
    let largest_regions = match vmmap::regions(&pid_arg) {
        Ok(text) => match memory::parse_regions(&text, top as usize) {
            Ok(regions) => available(regions.into_iter().map(|r| json!({"kind":r.kind,"address":r.address,"detail":r.detail,"memory":sizes(r.memory),"dirty_swapped_bytes":r.dirty_swapped_bytes})).collect()),
            Err(reason) => malformed(reason, &command),
        },
        Err(reason) => skipped(reason, &command),
    };
    let footprint = match footprint::read(&pid_arg) {
        Ok(text) => match memory::parse_footprint(&text) {
            Ok(bytes) => json!({"status":"available","bytes":bytes}),
            Err(reason) => malformed(reason, &command),
        },
        Err(reason) => skipped(reason, &command),
    };
    json!({"pid":pid,"top":top,"footprint":footprint,"region_kinds":region_kinds,"malloc_zones":malloc_zones,"largest_regions":largest_regions})
}
fn sizes(memory: Memory) -> Value {
    json!({"virtual_bytes":memory.virtual_bytes,"resident_bytes":memory.resident_bytes,"dirty_bytes":memory.dirty_bytes,"swapped_bytes":memory.swapped_bytes,"clean_bytes":memory.clean_bytes})
}
fn available(items: Vec<Value>) -> Value {
    json!({"status":"available","items":items})
}
fn malformed(reason: &str, command: &str) -> Value {
    json!({"status":"skipped","reason":format!("unrecognized Apple report: {reason}"),"fix":format!("brew upgrade sunfmin/tap/hostvitals && {command}")})
}
fn skipped(reason: String, command: &str) -> Value {
    let lower = reason.to_ascii_lowercase();
    let fix = if reason.contains("not found on PATH") {
        format!("PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH {command}")
    } else if [
        "permission",
        "not permitted",
        "task_for_pid",
        "cannot examine",
        "not allowed",
        "unable to get",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        format!("sudo {command}")
    } else {
        command.to_string()
    };
    json!({"status":"skipped","reason":reason,"fix":fix})
}
