//! Structured plist decoding and validation live here; only matched top-level drivers count.
use hostvitals_policy::disk::Counters;
use quick_xml::{Reader, events::Event};
use std::collections::BTreeMap;
use std::{path::PathBuf, process::Command, time::Duration};

fn args() -> Vec<&'static str> {
    vec!["-a", "-r", "-c", "IOBlockStorageDriver", "-d", "1"]
}

pub fn snapshot() -> Result<BTreeMap<u64, Counters>, String> {
    let text = super::capture(super::Tool::Ioreg, args())?;
    parse(text.as_bytes()).map_err(|e| format!("ioreg plist: {e}"))
}

pub(super) fn probe(path: PathBuf) -> Result<(), String> {
    let report = super::execute(
        Command::new(path).args(args()),
        super::MEASUREMENT_LIMIT_MIB * 1024 * 1024,
        Duration::from_secs(super::PROBE_TIMEOUT_SECONDS),
    )?;
    let counters = parse(&report.stdout)?;
    if counters
        .values()
        .all(|c| c.read_bytes.is_some() && c.written_bytes.is_some())
    {
        Ok(())
    } else {
        Err("ioreg has missing or invalid directional byte counters".into())
    }
}

fn parse(bytes: &[u8]) -> Result<BTreeMap<u64, Counters>, String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    if next(&mut reader)? != Tag::Start("plist".to_string()) {
        return Err("ioreg missing plist root".into());
    }
    let Tag::Start(name) = next(&mut reader)? else {
        return Err("ioreg missing inventory".into());
    };
    let report = value(&mut reader, &name, 0)?;
    if next(&mut reader)? != Tag::End("plist".to_string()) || next(&mut reader)? != Tag::Eof {
        return Err("ioreg truncated or trailing plist content".into());
    }
    let Node::Array(entries) = report else {
        return Err("ioreg inventory is not an array".into());
    };
    if entries.is_empty() {
        return Err("ioreg has no block-storage inventory".into());
    }
    let mut counters = BTreeMap::new();
    for entry in entries {
        let Node::Dict(entry) = entry else {
            return Err("ioreg entry is not a dictionary".into());
        };
        if !matches!(entry.get("IOObjectClass"), Some(Node::String(s)) if s == "IOBlockStorageDriver")
        {
            return Err("ioreg returned an unrecognized block-storage entry".into());
        }
        let id = integer(entry.get("IORegistryEntryID"))
            .filter(|n| *n > 0)
            .ok_or("ioreg entry has no unsigned registry identity")?;
        let stats = match entry.get("Statistics") {
            Some(Node::Dict(stats)) => Some(stats),
            _ => None,
        };
        let counter = |name: &str| integer(stats.and_then(|s| s.get(name)));
        let reading = Counters {
            read_bytes: counter("Bytes (Read)"),
            written_bytes: counter("Bytes (Write)"),
        };
        if counters.insert(id, reading).is_some() {
            return Err("ioreg has a duplicate registry identity".into());
        }
    }
    Ok(counters)
}

// Keep invalid numeric scalars typed but unknown, so one direction cannot poison the other.
// A generic plist decoder rejects an out-of-range integer before its field can be identified.
enum Node {
    Dict(BTreeMap<String, Node>),
    Array(Vec<Node>),
    String(String),
    Integer(Option<u64>),
    Other,
}
fn integer(node: Option<&Node>) -> Option<u64> {
    match node {
        Some(Node::Integer(n)) => *n,
        _ => None,
    }
}

#[derive(PartialEq)]
enum Tag {
    Start(String),
    End(String),
    Eof,
}

fn next(reader: &mut Reader<&[u8]>) -> Result<Tag, String> {
    loop {
        match reader.read_event().map_err(|e| format!("ioreg XML: {e}"))? {
            Event::Start(e) => return Ok(Tag::Start(e.name().as_ref().to_string())),
            Event::End(e) => return Ok(Tag::End(e.name().as_ref().to_string())),
            Event::Eof => return Ok(Tag::Eof),
            Event::Decl(_) | Event::DocType(_) | Event::Comment(_) => (),
            Event::Text(e) if e.as_ref().bytes().all(|b| b.is_ascii_whitespace()) => (),
            _ => return Err("ioreg unexpected plist content".into()),
        }
    }
}

fn value(reader: &mut Reader<&[u8]>, name: &str, depth: usize) -> Result<Node, String> {
    if depth >= 64 {
        return Err("ioreg plist nesting exceeds 64".into());
    }
    match name {
        "dict" => {
            let mut entries = BTreeMap::new();
            loop {
                match next(reader)? {
                    Tag::End(end) if end == name => break,
                    Tag::Start(key) if key == "key" => {
                        let key = text(reader, "key")?;
                        let Tag::Start(child) = next(reader)? else {
                            return Err("ioreg dictionary value missing".into());
                        };
                        let child = value(reader, &child, depth + 1)?;
                        if entries.insert(key, child).is_some() {
                            return Err("ioreg duplicate dictionary key".into());
                        }
                    }
                    _ => return Err("ioreg malformed dictionary".into()),
                }
            }
            Ok(Node::Dict(entries))
        }
        "array" => {
            let mut entries = Vec::new();
            loop {
                match next(reader)? {
                    Tag::End(end) if end == name => break,
                    Tag::Start(child) => entries.push(value(reader, &child, depth + 1)?),
                    _ => return Err("ioreg malformed array".into()),
                }
            }
            Ok(Node::Array(entries))
        }
        "string" => Ok(Node::String(text(reader, name)?)),
        "integer" => {
            let text = text(reader, name)?;
            let text = text.trim();
            let number = if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
                text.parse().ok()
            } else {
                None
            };
            Ok(Node::Integer(number))
        }
        "true" | "false" | "real" | "data" | "date" => {
            text(reader, name)?;
            Ok(Node::Other)
        }
        _ => Err("ioreg unrecognized plist value type".into()),
    }
}

fn text(reader: &mut Reader<&[u8]>, name: &str) -> Result<String, String> {
    let raw = reader
        .read_text(quick_xml::name::QName(name))
        .map_err(|e| format!("ioreg XML scalar: {e}"))?;
    if raw.contains('<') {
        return Err("ioreg nested scalar content".into());
    }
    quick_xml::escape::unescape(&raw)
        .map(|s| s.into_owned())
        .map_err(|e| format!("ioreg XML text: {e}"))
}
