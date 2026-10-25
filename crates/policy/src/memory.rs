//! Pure interpretation of Apple's text reports. Sizes are bytes, preserving reported precision.

#[derive(Debug)]
pub struct Memory {
    pub virtual_bytes: u64,
    pub resident_bytes: u64,
    pub dirty_bytes: u64,
    pub swapped_bytes: u64,
    pub clean_bytes: u64,
}
#[derive(Debug)]
pub struct RegionKind {
    pub kind: String,
    pub memory: Memory,
    pub regions: u64,
}
#[derive(Debug)]
pub struct MallocZone {
    pub zone: String,
    pub memory: Memory,
    pub allocations: u64,
    pub allocated_bytes: u64,
    pub regions: u64,
}
#[derive(Debug)]
pub struct Summary {
    pub kinds: Vec<RegionKind>,
    pub zones: Vec<MallocZone>,
}
#[derive(Debug)]
pub struct Region {
    pub kind: String,
    pub address: String,
    pub detail: String,
    pub memory: Memory,
    pub dirty_swapped_bytes: u64,
}

/// Reads the region-kind and malloc-zone tables; missing or malformed tables never mean zero.
pub fn parse_summary(text: &str) -> Result<Summary, &'static str> {
    let mut kinds = Vec::new();
    let mut zones = Vec::new();
    let mut table = 0;
    let mut kinds_complete = false;
    let mut zones_complete = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with("REGION TYPE") && line.contains("COUNT") {
            table = 1;
        } else if line.starts_with("MALLOC ZONE") {
            table = 2;
        } else if line.starts_with("TOTAL ") {
            if table == 1 {
                kinds_complete = true;
            }
            if table == 2 {
                zones_complete = true;
            }
            table = 0;
        } else if table != 0 && !line.is_empty() && !line.starts_with('=') {
            let words: Vec<_> = line.split_whitespace().collect();
            let first_size = words
                .iter()
                .position(|word| word.starts_with(|c: char| c.is_ascii_digit()))
                .ok_or("unrecognized vmmap table row")?;
            let name = words
                .get(..first_size)
                .ok_or("missing region name")?
                .join(" ");
            let values = words.get(first_size..).ok_or("missing vmmap columns")?;
            let memory = parse_memory(values)?;
            if table == 1 {
                kinds.push(RegionKind {
                    kind: name,
                    memory,
                    regions: count_at(values, 7)?,
                });
            } else {
                zones.push(MallocZone {
                    zone: name,
                    memory,
                    allocations: count_at(values, 4)?,
                    allocated_bytes: size_at(values, 5)?,
                    regions: count_at(values, 8)?,
                });
            }
        }
    }
    if !kinds_complete || !zones_complete || kinds.is_empty() {
        return Err("incomplete vmmap summary or malloc-zone table");
    }
    Ok(Summary { kinds, zones })
}

/// Reads wide vmmap regions and ranks by dirty plus swapped, then virtual size and address.
pub fn parse_regions(text: &str, top: usize) -> Result<Vec<Region>, &'static str> {
    let mut regions = Vec::new();
    for line in text.lines() {
        let Some((prefix, rest)) = line.split_once('[') else {
            continue;
        };
        let words: Vec<_> = prefix.split_whitespace().collect();
        let Some(address) = words.last() else {
            continue;
        };
        let Some((start, end)) = address.split_once('-') else {
            continue;
        };
        if *address != "kernel-kernel"
            && (start.is_empty()
                || end.is_empty()
                || !start.chars().all(|c| c.is_ascii_hexdigit())
                || !end.chars().all(|c| c.is_ascii_hexdigit()))
        {
            continue;
        }
        let (sizes, detail) = rest.split_once(']').ok_or("incomplete vmmap region")?;
        let sizes: Vec<_> = sizes.split_whitespace().collect();
        let memory = parse_memory(&sizes)?;
        let dirty_swapped_bytes = memory
            .dirty_bytes
            .checked_add(memory.swapped_bytes)
            .ok_or("region size overflow")?;
        let kind_words = words.len().checked_sub(1).ok_or("missing region kind")?;
        let kind = words
            .get(..kind_words)
            .ok_or("missing region kind")?
            .join(" ");
        regions.push(Region {
            kind,
            address: address.to_string(),
            detail: detail.trim().to_string(),
            memory,
            dirty_swapped_bytes,
        });
    }
    if regions.is_empty() {
        return Err("no wide vmmap regions found");
    }
    regions.sort_by(|a, b| {
        b.dirty_swapped_bytes
            .cmp(&a.dirty_swapped_bytes)
            .then(b.memory.virtual_bytes.cmp(&a.memory.virtual_bytes))
            .then(a.address.cmp(&b.address))
    });
    regions.truncate(top);
    Ok(regions)
}

/// Reads the exact charged-memory total from footprint's bytes-format auxiliary data.
pub fn parse_footprint(text: &str) -> Result<u64, &'static str> {
    for line in text.lines() {
        if let Some(value) = line.trim().strip_prefix("phys_footprint:") {
            return size(value.trim());
        }
    }
    Err("footprint report has no phys_footprint total")
}

fn parse_memory(values: &[&str]) -> Result<Memory, &'static str> {
    let virtual_bytes = size_at(values, 0)?;
    let resident_bytes = size_at(values, 1)?;
    let dirty_bytes = size_at(values, 2)?;
    let swapped_bytes = size_at(values, 3)?;
    let clean_bytes = resident_bytes
        .checked_sub(dirty_bytes)
        .ok_or("dirty memory exceeds resident memory")?;
    Ok(Memory {
        virtual_bytes,
        resident_bytes,
        dirty_bytes,
        swapped_bytes,
        clean_bytes,
    })
}
fn size_at(values: &[&str], index: usize) -> Result<u64, &'static str> {
    size(values.get(index).ok_or("missing vmmap size column")?)
}
fn count_at(values: &[&str], index: usize) -> Result<u64, &'static str> {
    values
        .get(index)
        .ok_or("missing vmmap count column")?
        .parse()
        .map_err(|_| "invalid vmmap count")
}

/// Apple uses binary K/M/G/T units, sometimes decimal, and bare byte counts.
fn size(text: &str) -> Result<u64, &'static str> {
    let text = text.trim();
    let (number, factor) = match text.chars().last() {
        Some('B') => (text.strip_suffix('B').ok_or("invalid size")?.trim(), 1_u64),
        Some('K') => (text.strip_suffix('K').ok_or("invalid size")?, 1024),
        Some('M') => (text.strip_suffix('M').ok_or("invalid size")?, 1048576),
        Some('G') => (text.strip_suffix('G').ok_or("invalid size")?, 1073741824),
        Some('T') => (text.strip_suffix('T').ok_or("invalid size")?, 1099511627776),
        _ => (text, 1),
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let whole: u64 = whole.parse().map_err(|_| "invalid memory size")?;
    let mut numerator = 0_u64;
    let mut denominator = 1_u64;
    for digit in fraction.chars() {
        let digit = digit.to_digit(10).ok_or("invalid fractional size")?;
        numerator = numerator
            .checked_mul(10)
            .and_then(|n| n.checked_add(u64::from(digit)))
            .ok_or("memory size overflow")?;
        denominator = denominator.checked_mul(10).ok_or("memory size overflow")?;
    }
    let fractional = numerator
        .checked_mul(factor)
        .ok_or("memory size overflow")?
        .checked_div(denominator)
        .ok_or("invalid memory size")?;
    whole
        .checked_mul(factor)
        .and_then(|n| n.checked_add(fractional))
        .ok_or("memory size overflow")
}
