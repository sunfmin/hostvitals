//! One deadline for every reverse lookup, including subprocess cleanup.
use super::Tool;
use std::{
    collections::BTreeMap,
    process::Command,
    time::{Duration, Instant},
};
pub(crate) const DEADLINE_SECONDS: u64 = 2;
pub(super) fn args(ip: &str) -> Vec<&str> {
    vec!["-q", "host", "-a", "ip_address", ip]
}

/// Missing, failed, oversized and timed-out lookups all leave the literal IP intact.
pub fn resolve(ips: impl Iterator<Item = String>) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    let deadline = Instant::now() + Duration::from_secs(DEADLINE_SECONDS);
    let path = Tool::Dscacheutil.find();
    for ip in ips {
        if names.contains_key(&ip) {
            continue;
        }
        let name = path
            .as_ref()
            .and_then(|path| {
                let remaining = deadline.checked_duration_since(Instant::now())?;
                let bytes =
                    super::execute(Command::new(path).args(args(&ip)), 64 * 1024, remaining)
                        .ok()?;
                let text = String::from_utf8(bytes.stdout).ok()?;
                text.lines().find_map(|line| {
                    line.strip_prefix("name:")
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                })
            })
            .unwrap_or_else(|| ip.clone());
        names.insert(ip, name);
    }
    names
}
