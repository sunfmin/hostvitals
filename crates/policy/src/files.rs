//! Pure interpretation of lsof fields and fs_usage filesystem events.
pub struct Inventory {
    pub cwd: Option<String>,
    pub files: Vec<File>,
    pub sockets: Vec<Socket>,
}
pub struct File {
    pub fd: String,
    pub kind: String,
    pub path: String,
}
pub struct Socket {
    pub fd: String,
    pub kind: String,
    pub protocol: Option<String>,
    pub local: String,
    pub remote_ip: Option<String>,
    pub remote_port: Option<u16>,
    pub state: Option<String>,
}

/// Rejects absent/mismatched process headers and incomplete file records instead of inventing emptiness.
pub fn parse(text: &str, pid: u32) -> Result<Inventory, &'static str> {
    let mut result = Inventory {
        cwd: None,
        files: Vec::new(),
        sockets: Vec::new(),
    };
    let mut seen = false;
    let mut fields = Vec::new();
    for field in text.split('\0') {
        let field = field.strip_prefix('\n').unwrap_or(field);
        if field.is_empty() {
            continue;
        }
        if let Some(value) = field.strip_prefix('p') {
            if seen || value.parse::<u32>().ok() != Some(pid) {
                return Err("invalid lsof process identity");
            }
            seen = true;
        } else if field.starts_with('f') {
            if !seen {
                return Err("lsof file before process identity");
            }
            if !fields.is_empty() {
                record(&fields, &mut result)?;
            }
            fields.clear();
            fields.push(field);
        } else {
            fields.push(field);
        }
    }
    if !seen {
        return Err("missing lsof process identity");
    }
    if !fields.is_empty() {
        record(&fields, &mut result)?;
    }
    Ok(result)
}
fn record(fields: &[&str], result: &mut Inventory) -> Result<(), &'static str> {
    let field = |prefix: char| fields.iter().find_map(|s| s.strip_prefix(prefix));
    let fd = field('f').ok_or("missing lsof descriptor")?;
    let kind = field('t').ok_or("missing lsof file kind")?;
    let name = field('n').ok_or("missing lsof file name (access denied or unsupported file)")?;
    if fd == "cwd" {
        result.cwd = Some(name.into());
    } else if ["IPv4", "IPv6", "unix", "sock"].contains(&kind) {
        let (local, remote) = if kind == "IPv4" || kind == "IPv6" {
            name.split_once("->")
                .map_or((name, None), |(l, r)| (l, Some(r)))
        } else {
            (name, None)
        };
        let endpoint = remote
            .map(|text| endpoint(text).ok_or("unrecognized lsof remote endpoint"))
            .transpose()?;
        result.sockets.push(Socket {
            fd: fd.into(),
            kind: kind.into(),
            protocol: field('P').map(str::to_owned),
            local: local.into(),
            remote_ip: endpoint.as_ref().map(|(ip, _)| ip.clone()),
            remote_port: endpoint.map(|(_, port)| port),
            state: fields
                .iter()
                .find_map(|f| f.strip_prefix("TST="))
                .map(str::to_owned),
        });
    } else {
        result.files.push(File {
            fd: fd.into(),
            kind: kind.into(),
            path: name.into(),
        });
    }
    Ok(())
}
fn endpoint(text: &str) -> Option<(String, u16)> {
    let (ip, port) = text.rsplit_once(':')?;
    let ip = ip
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(ip);
    ip.split('%').next()?.parse::<std::net::IpAddr>().ok()?;
    Some((ip.into(), port.parse().ok()?))
}

pub struct Activity {
    pub operation: String,
    pub path: Option<String>,
    pub bytes: Option<u64>,
}
/// Keeps read/write events in source order; requested byte counts are not successful-byte accounting.
pub fn activity(text: &str, initial_files: &[File]) -> Result<Vec<Activity>, &'static str> {
    let mut answer = Vec::new();
    let mut paths = std::collections::BTreeMap::new();
    for file in initial_files {
        paths.insert(file.fd.clone(), file.path.clone());
    }
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let words: Vec<_> = line.split_whitespace().collect();
        let time = words.first().ok_or("missing fs_usage timestamp")?;
        if !timestamp(time) {
            return Err("unrecognized fs_usage report or dropped events");
        }
        let operation = words.get(1).ok_or("missing fs_usage operation")?;
        let lower = operation.to_ascii_lowercase();
        // Failed syscalls neither accessed the file nor changed a descriptor association.
        if words.iter().any(|w| w.starts_with('[') && w.ends_with(']')) {
            continue;
        }
        let fd = words.iter().find_map(|w| w.strip_prefix("F="));
        let bytes = words
            .iter()
            .find_map(|w| w.strip_prefix("B="))
            .map(|n| {
                let n = n.strip_prefix("0x").unwrap_or(n);
                u64::from_str_radix(n, 16).map_err(|_| "invalid fs_usage byte count")
            })
            .transpose()?;
        // Remove the process/thread and duration from the right, preserving path spaces.
        let mut body = line.trim_end();
        for _ in 0..2 {
            body = body
                .rsplit_once(char::is_whitespace)
                .ok_or("missing fs_usage duration/process")?
                .0
                .trim_end();
            if body.ends_with(" W") {
                body = body.strip_suffix(" W").unwrap_or(body).trim_end();
            }
        }
        let observed_path = body
            .find('/')
            .and_then(|i| body.get(i..))
            .map(str::to_owned);
        if lower.starts_with("open") {
            if let Some(fd) = fd {
                paths.remove(fd);
                if let Some(path) = observed_path {
                    paths.insert(fd.into(), path);
                }
            }
            continue;
        }
        if lower.starts_with("close") {
            if let Some(fd) = fd {
                paths.remove(fd);
            }
            continue;
        }
        // A duplicate can replace a descriptor without a pathname in this format.
        if lower.starts_with("dup") || lower == "fcntl" {
            paths.clear();
            continue;
        }
        if !(lower.starts_with("read")
            || lower.starts_with("pread")
            || lower.starts_with("write")
            || lower.starts_with("pwrite")
            || lower.starts_with("rddata")
            || lower.starts_with("wrdata"))
        {
            continue;
        }
        let path = observed_path.or_else(|| fd.and_then(|fd| paths.get(fd).cloned()));
        answer.push(Activity {
            operation: (*operation).into(),
            path,
            bytes,
        });
    }
    Ok(answer)
}

fn timestamp(text: &str) -> bool {
    let (clock, fraction) = text.split_once('.').unwrap_or((text, "0"));
    clock.split(':').count() == 3
        && clock
            .split(':')
            .all(|part| part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
        && !fraction.is_empty()
        && fraction.bytes().all(|b| b.is_ascii_digit())
}
