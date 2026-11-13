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

/// What one df run yields: the volumes of every readable row, and each row
/// that could not be read, so one odd row never hides the rest.
pub struct Inventory {
    pub volumes: Vec<Volume>,
    pub skipped_rows: Vec<SkippedRow>,
}

/// A df row left out of the inventory, unreadable or a duplicate, with its mount point when it was read.
pub struct SkippedRow {
    pub reason: &'static str,
    pub mount_point: Option<String>,
}

/// Fails only when df fails, its header is unrecognized, or no row is readable.
pub fn snapshot() -> Result<Inventory, String> {
    let text = super::capture(super::Tool::Df, args())?;
    parse(&text).map_err(str::to_owned)
}

fn parse(text: &str) -> Result<Inventory, &'static str> {
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
    let mut inventory = Inventory {
        volumes: Vec::new(),
        skipped_rows: Vec::new(),
    };
    let mut identities = BTreeSet::new();
    for line in lines {
        let Some(row) = Row::read(line) else {
            inventory.skipped_rows.push(SkippedRow {
                reason: "unreadable df volume row: no 1024-blocks, Used, Available and Capacity columns followed by an absolute mount point",
                mount_point: None,
            });
            continue;
        };
        if !identities.insert((row.filesystem, row.mount_point)) {
            inventory.skipped_rows.push(SkippedRow {
                reason: "duplicate df mount identity: the same filesystem and mount point were reported twice",
                mount_point: Some(row.mount_point.into()),
            });
            continue;
        }
        let size_bytes = bytes(row.blocks);
        let mut free_bytes = bytes(row.available);
        if size_bytes
            .zip(free_bytes)
            .is_some_and(|(size, free)| free > size)
        {
            free_bytes = None;
        }
        inventory.volumes.push(Volume {
            filesystem: row.filesystem.into(),
            mount_point: row.mount_point.into(),
            size_bytes,
            free_bytes,
            free_share: hostvitals_policy::disk::free_share(size_bytes, free_bytes),
        });
    }
    if inventory.volumes.is_empty() {
        return Err(if inventory.skipped_rows.is_empty() {
            "df returned no mounted local volumes"
        } else {
            "df returned no readable mounted local volume row"
        });
    }
    Ok(inventory)
}

/// One df row's columns. Filesystem names and mount points may both hold
/// spaces, so the row is anchored on its numeric columns: the first word that
/// reads as a Capacity (`NN%` or `-`), preceded by three block columns and at
/// least one filesystem word, and followed by an absolute mount point.
struct Row<'a> {
    filesystem: &'a str,
    blocks: &'a str,
    available: &'a str,
    mount_point: &'a str,
}

impl<'a> Row<'a> {
    fn read(line: &'a str) -> Option<Self> {
        let words: Vec<(usize, &str)> = line
            .split_whitespace()
            .map(|w| (w.as_ptr() as usize - line.as_ptr() as usize, w))
            .collect();
        (4..words.len()).find_map(|capacity| {
            let (at, word) = words[capacity];
            let mount_point = line[at + word.len()..].trim_start();
            let columns = &words[capacity - 3..capacity];
            (is_capacity(word)
                && mount_point.starts_with('/')
                && columns.iter().all(|(_, w)| is_block_count(w)))
            .then(|| Row {
                filesystem: line[..columns[0].0].trim(),
                blocks: columns[0].1,
                available: columns[2].1,
                mount_point,
            })
        })
    }
}

fn is_block_count(word: &str) -> bool {
    word == "-" || word.bytes().all(|b| b.is_ascii_digit())
}

fn is_capacity(word: &str) -> bool {
    word == "-"
        || word
            .strip_suffix('%')
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn bytes(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok()?.checked_mul(1024)
}
