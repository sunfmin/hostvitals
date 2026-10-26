//! Local mounted volume capacity comes from df, independently of activity counters.
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Serialize)]
pub struct Volume {
    pub filesystem: String,
    pub mount_point: String,
    pub size_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub free_share: Option<f64>,
}

pub(super) fn args() -> Vec<&'static str> {
    vec!["-a", "-l", "-k", "-P"]
}

pub fn snapshot() -> Result<Vec<Volume>, String> {
    let text = super::capture(super::Tool::Df, args())?;
    parse(&text).map_err(str::to_owned)
}

fn parse(text: &str) -> Result<Vec<Volume>, &'static str> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    if lines
        .next()
        .map(|h| h.split_whitespace().collect::<Vec<_>>())
        != Some(vec![
            "Filesystem",
            "1024-blocks",
            "Used",
            "Available",
            "Capacity",
            "Mounted",
            "on",
        ])
    {
        return Err("unrecognized df local-volume header");
    }
    let mut volumes = Vec::new();
    let mut identities = BTreeSet::new();
    for line in lines {
        let mut rest = line.trim_start();
        let mut fields = Vec::new();
        for _ in 0..5 {
            let end = rest
                .find(char::is_whitespace)
                .ok_or("truncated df volume row")?;
            fields.push(&rest[..end]);
            rest = rest[end..].trim_start();
        }
        if !rest.starts_with('/') || !identities.insert((fields[0], rest)) {
            return Err("invalid or duplicate df mount identity");
        }
        let size_bytes = bytes(fields[1]);
        let mut free_bytes = bytes(fields[3]);
        if size_bytes
            .zip(free_bytes)
            .is_some_and(|(size, free)| free > size)
        {
            free_bytes = None;
        }
        volumes.push(Volume {
            filesystem: fields[0].into(),
            mount_point: rest.into(),
            size_bytes,
            free_bytes,
            free_share: hostvitals_policy::disk::free_share(size_bytes, free_bytes),
        });
    }
    if volumes.is_empty() {
        return Err("df returned no mounted local volumes");
    }
    Ok(volumes)
}

fn bytes(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok()?.checked_mul(1024)
}
