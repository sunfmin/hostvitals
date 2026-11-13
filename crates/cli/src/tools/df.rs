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
        let row = row(line).ok_or("unrecognized df volume row")?;
        if !identities.insert((row.filesystem, row.mount_point)) {
            return Err("invalid or duplicate df mount identity");
        }
        let size_bytes = bytes(row.size);
        let mut free_bytes = bytes(row.available);
        if size_bytes
            .zip(free_bytes)
            .is_some_and(|(size, free)| free > size)
        {
            free_bytes = None;
        }
        volumes.push(Volume {
            filesystem: row.filesystem.into(),
            mount_point: row.mount_point.into(),
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

struct Row<'a> {
    filesystem: &'a str,
    size: &'a str,
    available: &'a str,
    mount_point: &'a str,
}

/// `df -P` pads its columns and quotes no name, so a Filesystem name and a
/// mount point may both hold spaces. A row is read around its first run of
/// four measurements (`blocks used available capacity%`, each `-` when
/// unknown) that is followed by an absolute mount point: what precedes the run
/// is the Filesystem, what follows it is the mount point, each kept whole.
fn row(line: &str) -> Option<Row<'_>> {
    let words: Vec<(usize, &str)> = line
        .split_whitespace()
        .map(|w| (w.as_ptr() as usize - line.as_ptr() as usize, w))
        .collect();
    let count = |w: &str| w == "-" || w.bytes().all(|b| b.is_ascii_digit());
    (1..words.len().saturating_sub(4)).find_map(|i| {
        let [
            (_, size),
            (_, used),
            (_, available),
            (_, capacity),
            (mount_at, mount),
        ] = words[i..i + 5]
        else {
            return None;
        };
        let share = capacity == "-"
            || capacity
                .strip_suffix('%')
                .is_some_and(|n| !n.is_empty() && count(n));
        if !(count(size) && count(used) && count(available) && share && mount.starts_with('/')) {
            return None;
        }
        let (start, _) = words[0];
        let (end, last) = words[i - 1];
        Some(Row {
            filesystem: &line[start..end + last.len()],
            size,
            available,
            mount_point: &line[mount_at..],
        })
    })
}

fn bytes(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok()?.checked_mul(1024)
}

#[cfg(test)]
mod tests {
    use super::{Volume, parse};

    const HEADER: &str = "Filesystem 1024-blocks Used Available Capacity Mounted on\n";

    fn only(row: &str) -> Volume {
        let mut volumes = parse(&format!("{HEADER}{row}\n")).unwrap();
        assert_eq!(volumes.len(), 1);
        volumes.remove(0)
    }

    #[test]
    fn a_filesystem_name_with_spaces_is_read_whole() {
        let v = only(
            "devices -- file:///Users/someone/Library/Containers/com.apple.CoreDevice.CoreDeviceService/Data/  1073741824         0 1073741824     0%    /Users/someone/Library/Developer/CoreDevice/DeviceFS",
        );
        assert_eq!(
            v.filesystem,
            "devices -- file:///Users/someone/Library/Containers/com.apple.CoreDevice.CoreDeviceService/Data/"
        );
        assert_eq!(
            v.mount_point,
            "/Users/someone/Library/Developer/CoreDevice/DeviceFS"
        );
        assert_eq!(v.size_bytes, Some(1073741824 * 1024));
        assert_eq!(v.free_bytes, Some(1073741824 * 1024));
    }

    #[test]
    fn a_mount_point_with_spaces_is_read_whole() {
        let v = only("/dev/disk4s1  1000  400  600    40%    /Volumes/My  Disk 2");
        assert_eq!(v.filesystem, "/dev/disk4s1");
        assert_eq!(v.mount_point, "/Volumes/My  Disk 2");
        assert_eq!(v.size_bytes, Some(1024000));
        assert_eq!(v.free_bytes, Some(614400));
    }

    #[test]
    fn spaces_on_both_sides_are_told_apart_by_the_measurement_run() {
        let v = only("//someone@nas/Shared Media 2  2000 1000 1000 50% /Volumes/Shared Media 2");
        assert_eq!(v.filesystem, "//someone@nas/Shared Media 2");
        assert_eq!(v.mount_point, "/Volumes/Shared Media 2");
        assert_eq!(v.size_bytes, Some(2048000));
    }

    #[test]
    fn unknown_measurements_stay_unknown() {
        let v = only("/dev/unknown - - - - /Volumes/unknown");
        assert_eq!(v.mount_point, "/Volumes/unknown");
        assert_eq!((v.size_bytes, v.free_bytes), (None, None));
    }

    #[test]
    fn a_captured_report_with_the_coredevice_mount_reads_every_volume() {
        let volumes = parse(include_str!("../../tests/fixtures/df-coredevice.txt")).unwrap();
        assert_eq!(volumes.len(), 15);
        assert!(volumes.iter().any(|v| v.mount_point == "/"));
        let device = volumes
            .iter()
            .find(|v| v.mount_point == "/Users/someone/Library/Developer/CoreDevice/DeviceFS")
            .unwrap();
        assert!(device.filesystem.starts_with("devices -- file:///"));
        assert!(volumes.iter().all(|v| v.size_bytes.is_some()));
    }

    #[test]
    fn a_row_with_no_measurement_run_or_no_absolute_mount_is_refused() {
        for row in [
            "truncated",
            "/dev/a 1000 500 500 50%",
            "/dev/a 1000 500 500 50% relative",
        ] {
            assert!(parse(&format!("{HEADER}{row}\n")).is_err(), "{row}");
        }
    }
}
