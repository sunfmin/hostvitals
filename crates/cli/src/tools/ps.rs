//! Entitled process inventory, cumulative CPU and resident memory from ps.
use std::collections::BTreeMap;
pub(super) fn args(pid: &str) -> Vec<&str> {
    vec!["-p", pid, "-o", "pid=,pcpu=,rss="]
}

/// Reads every user's cumulative CPU and resident memory, without command arguments.
pub(crate) fn snapshot() -> Result<BTreeMap<u32, SnapshotProcess>, String> {
    let path = super::Tool::Ps
        .find()
        .ok_or("ps missing; run PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH hostvitals now")?;
    super::run(
        std::process::Command::new(path)
            .args([
                "-Aww",
                "-o",
                "pid=,ppid=,uid=,user=,time=,rss=,lstart=,comm=",
            ])
            .env("LC_ALL", "C")
            .env("TZ", "UTC"),
        true,
    )
    .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
    .map_err(|e| format!("ps snapshot: {e}; run hostvitals doctor && hostvitals now"))
    .and_then(|text| parse(&text))
}

pub(crate) struct SnapshotProcess {
    pub pid: u32,
    pub parent_pid: u32,
    pub uid: u32,
    pub user: String,
    pub name: String,
    pub started_at: String,
    pub cpu_hundredths: u64,
    pub resident_bytes: u64,
}
fn parse(text: &str) -> Result<BTreeMap<u32, SnapshotProcess>, String> {
    let mut processes = BTreeMap::new();
    for (index, line) in text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
    {
        let parsed = (|| {
            let mut rest = line.trim();
            let mut fields = Vec::new();
            for _ in 0..11 {
                let end = rest.find(char::is_whitespace).ok_or("missing ps field")?;
                fields.push(&rest[..end]);
                rest = rest[end..].trim_start();
            }
            let p = SnapshotProcess {
                pid: fields[0].parse().map_err(|_| "invalid pid")?,
                parent_pid: fields[1].parse().map_err(|_| "invalid parent pid")?,
                uid: fields[2].parse().map_err(|_| "invalid uid")?,
                user: fields[3].into(),
                cpu_hundredths: cpu_time(fields[4]).ok_or("invalid cumulative CPU time")?,
                resident_bytes: fields[5]
                    .parse::<u64>()
                    .ok()
                    .and_then(|n| n.checked_mul(1024))
                    .ok_or("invalid resident memory")?,
                started_at: fields[6..11].join(" "),
                name: rest
                    .rsplit('/')
                    .next()
                    .filter(|s| !s.is_empty())
                    .ok_or("missing name")?
                    .into(),
            };
            if p.pid == 0 || processes.insert(p.pid, p).is_some() {
                return Err("invalid or duplicate process identity");
            }
            Ok(())
        })();
        parsed.map_err(|e| {
            format!(
                "ps snapshot line {}: {e}; run hostvitals doctor && hostvitals now",
                index + 1
            )
        })?;
    }
    if processes.is_empty() {
        return Err("ps returned no processes; run hostvitals doctor && hostvitals now".into());
    }
    Ok(processes)
}
// ps emits [[days-]hours:]minutes:seconds.hundredths, with unbounded minutes.
fn cpu_time(text: &str) -> Option<u64> {
    let (days, clock) = match text.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, text),
    };
    let (clock, fraction) = clock.split_once('.').unwrap_or((clock, "0"));
    if fraction.len() > 2 || fraction.is_empty() || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut seconds = 0u64;
    let mut count = 0;
    for part in clock.split(':') {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        seconds = seconds.checked_mul(60)?.checked_add(part.parse().ok()?)?;
        count += 1;
    }
    if !(2..=3).contains(&count) {
        return None;
    }
    let fraction = fraction
        .parse::<u64>()
        .ok()?
        .checked_mul(if fraction.len() == 1 { 10 } else { 1 })?;
    days.checked_mul(86400)?
        .checked_add(seconds)?
        .checked_mul(100)?
        .checked_add(fraction)
}
