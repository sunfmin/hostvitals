//! Per-process network accounting; unknown counters never become measured zeroes.
use std::collections::{BTreeMap, BTreeSet};

pub struct Counters {
    bytes_in: u64,
    bytes_out: u64,
}

pub struct Change {
    pub bytes_in: Option<u64>,
    pub bytes_out: Option<u64>,
}

pub struct Changes {
    pub processes: BTreeMap<u32, Change>,
    pub host: Change,
}

/// Joins the byte readings to verified Process identities and keeps the host sum complete.
/// `identities` includes the final inventory; true means its PID and start time matched initially.
/// A nettop-only PID still participates in the host total, with an unknown change.
pub fn changes(
    before: &BTreeMap<u32, Counters>,
    after: &BTreeMap<u32, Counters>,
    identities: &BTreeMap<u32, bool>,
) -> Changes {
    let network_pids: BTreeSet<_> = before.keys().chain(after.keys()).copied().collect();
    let all_pids: BTreeSet<_> = network_pids
        .iter()
        .chain(identities.keys())
        .copied()
        .collect();
    let mut processes = BTreeMap::new();
    let mut host = Change {
        bytes_in: Some(0),
        bytes_out: Some(0),
    };
    for pid in all_pids {
        let same = identities.get(&pid).copied().unwrap_or(false);
        let delta = change(before.get(&pid), after.get(&pid), same);
        if network_pids.contains(&pid) {
            host.bytes_in = super::add_bytes(host.bytes_in, delta.bytes_in);
            host.bytes_out = super::add_bytes(host.bytes_out, delta.bytes_out);
        }
        processes.insert(pid, delta);
    }
    Changes { processes, host }
}

/// Parses nettop's process-only, exact-byte report, including names containing commas or dots.
/// A header alone means no sockets; an absent header or invalid row is not an empty reading.
pub fn parse(text: &str) -> Result<BTreeMap<u32, Counters>, &'static str> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let header = lines.next().ok_or("missing nettop header")?;
    let header = header.trim();
    let reversed = match header.strip_suffix(',').unwrap_or(header) {
        ",bytes_in,bytes_out" => false,
        ",bytes_out,bytes_in" => true,
        _ => return Err("unrecognized nettop byte columns"),
    };
    let mut counters = BTreeMap::new();
    for line in lines {
        let line = line.trim_end();
        let mut fields = line.strip_suffix(',').unwrap_or(line).rsplitn(3, ',');
        let last = number(fields.next().ok_or("missing nettop byte counter")?)?;
        let first = number(fields.next().ok_or("missing nettop byte counter")?)?;
        let identity = fields.next().ok_or("missing nettop process identity")?;
        let (name, pid) = identity
            .rsplit_once('.')
            .ok_or("invalid nettop process identity")?;
        if name.is_empty() {
            return Err("missing nettop process name");
        }
        let pid = u32::try_from(number(pid)?).map_err(|_| "invalid nettop pid")?;
        let reading = if reversed {
            Counters {
                bytes_in: last,
                bytes_out: first,
            }
        } else {
            Counters {
                bytes_in: first,
                bytes_out: last,
            }
        };
        if counters.insert(pid, reading).is_some() {
            return Err("duplicate nettop pid");
        }
    }
    Ok(counters)
}

fn number(text: &str) -> Result<u64, &'static str> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err("invalid nettop integer");
    }
    text.parse().map_err(|_| "nettop integer overflow")
}

/// No sockets at either reading is zero only for a Process observed throughout the Window.
/// One missing reading or changed identity is unknown; a regression affects its direction only.
fn change(before: Option<&Counters>, after: Option<&Counters>, same_process: bool) -> Change {
    match (same_process, before, after) {
        (true, Some(before), Some(after)) => Change {
            bytes_in: super::counter_delta(before.bytes_in, after.bytes_in, true),
            bytes_out: super::counter_delta(before.bytes_out, after.bytes_out, true),
        },
        (true, None, None) => Change {
            bytes_in: Some(0),
            bytes_out: Some(0),
        },
        _ => Change {
            bytes_in: None,
            bytes_out: None,
        },
    }
}

/// Converts a byte change using the caller's positive, finite monotonic interval.
pub fn per_second(bytes: Option<u64>, seconds: f64) -> Option<f64> {
    bytes.map(|bytes| bytes as f64 / seconds)
}
