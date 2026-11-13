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
        let (filesystem, counts, mount_point) = row(line)?;
        if !identities.insert((filesystem, mount_point)) {
            return Err("invalid or duplicate df mount identity");
        }
        let size_bytes = bytes(counts[0]);
        let mut free_bytes = bytes(counts[2]);
        if size_bytes
            .zip(free_bytes)
            .is_some_and(|(size, free)| free > size)
        {
            free_bytes = None;
        }
        volumes.push(Volume {
            filesystem: filesystem.into(),
            mount_point: mount_point.into(),
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

/// Splits a `df -P` row into its filesystem, its blocks, used and available
/// counts, and its mount point. Both the filesystem and the mount point may
/// hold spaces (CoreDevice mounts `devices -- file:///…`, a volume named
/// `/Volumes/low space`), so the row is split where the first run of three
/// counts and a capacity is followed by an absolute mount point.
fn row(line: &str) -> Result<(&str, [&str; 3], &str), &'static str> {
    let words: Vec<(usize, &str)> = line
        .split_whitespace()
        .map(|w| (w.as_ptr() as usize - line.as_ptr() as usize, w))
        .collect();
    if words.len() < 6 {
        return Err("truncated df volume row");
    }
    let count = |w: &str| w == "-" || (!w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()));
    let capacity = |w: &str| w == "-" || count(w.strip_suffix('%').unwrap_or(w));
    (1..words.len() - 4)
        .find(|&i| {
            words[i..i + 3].iter().all(|(_, w)| count(w))
                && capacity(words[i + 3].1)
                && words[i + 4].1.starts_with('/')
        })
        .map(|i| {
            let (last, word) = words[i - 1];
            (
                &line[words[0].0..last + word.len()],
                [words[i].1, words[i + 1].1, words[i + 2].1],
                &line[words[i + 4].0..],
            )
        })
        .ok_or("invalid or duplicate df mount identity")
}

fn bytes(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok()?.checked_mul(1024)
}
