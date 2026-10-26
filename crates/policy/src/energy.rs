//! Energy scores and component power are different observations, never interchangeable.
use std::collections::BTreeMap;

/// Discards top's initial zero sample and accepts only a complete final PID/POWER table.
pub fn parse_top(text: &str) -> Result<BTreeMap<u32, f64>, &'static str> {
    let mut readings = 0u8;
    let mut scores = BTreeMap::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(first) = fields.next() else { continue };
        if first == "PID" && fields.next() == Some("POWER") {
            if readings >= 2 {
                return Err("too many top samples");
            }
            readings += 1;
            scores.clear();
        } else if readings > 0 && first != "Processes:" {
            // Headers of the next sample precede its PID table.
            if !first.bytes().all(|b| b.is_ascii_digit()) {
                if readings == 2 {
                    return Err("unrecognized final top row");
                }
                continue;
            }
            let pid = first.parse().map_err(|_| "invalid top PID")?;
            let value = fields.next().ok_or("missing POWER")?;
            let score = nonnegative(value)?;
            if fields.next().is_some() || scores.insert(pid, score).is_some() {
                return Err("invalid or duplicate top row");
            }
        }
    }
    if readings != 2 || scores.is_empty() {
        return Err("expected two nonempty top PID/POWER samples");
    }
    Ok(scores)
}

fn nonnegative(text: &str) -> Result<f64, &'static str> {
    let value: f64 = text.parse().map_err(|_| "invalid numeric energy reading")?;
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err("energy reading is negative or nonfinite")
    }
}

/// IOReport energy units are explicit; absent/invalid channels never become zero watts.
pub fn watts(value: i64, unit: &str, seconds: f64) -> Option<f64> {
    let scale = match unit {
        "mJ" => 1e3,
        "uJ" => 1e6,
        "nJ" => 1e9,
        "J" => 1.0,
        _ => return None,
    };
    if value < 0 || !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let watts = value as f64 / (scale * seconds);
    if watts.is_finite() { Some(watts) } else { None }
}

/// No accumulated energy in either raw sample cannot distinguish an idle channel from a restricted one.
/// A zero delta at a positive baseline is an observed idle zero; resets are unknown.
pub fn counter_watts(
    value: i64,
    before: Option<i64>,
    after: Option<i64>,
    unit: &str,
    seconds: f64,
) -> Option<f64> {
    match (before, after) {
        (Some(b), Some(a)) if b >= 0 && a >= b && a > 0 && a - b == value => {
            watts(value, unit, seconds)
        }
        (None, None) => watts(value, unit, seconds), // A fixture may supply an already validated delta.
        _ => None,
    }
}

/// GPU activity is active residency divided by all residency, independent of its tick unit.
pub fn gpu_active_share(states: &[(String, i64)]) -> Option<f64> {
    let mut total = 0u64;
    let mut active = 0u64;
    for (name, value) in states {
        if *value < 0 {
            return None;
        }
        total = total.checked_add(*value as u64)?;
        if !matches!(name.as_str(), "OFF" | "IDLE" | "DOWN") {
            active = active.checked_add(*value as u64)?;
        }
    }
    if total == 0 {
        None
    } else {
        Some(active as f64 / total as f64)
    }
}

pub struct Battery {
    pub present: bool,
    pub charge_percent: Option<u32>,
    pub charging: Option<bool>,
    pub state: Option<String>,
    pub power_source: String,
    pub time_remaining_minutes: Option<u32>,
}

/// A source-only report is a Mac with no battery; an unknown battery row is an error.
pub fn parse_battery(text: &str) -> Result<Battery, &'static str> {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let source = lines
        .next()
        .and_then(|line| line.strip_prefix("Now drawing from '"))
        .and_then(|line| line.strip_suffix('\''))
        .filter(|s| !s.is_empty())
        .ok_or("unrecognized battery power source")?;
    let mut battery = Battery {
        present: false,
        charge_percent: None,
        charging: None,
        state: None,
        power_source: source.into(),
        time_remaining_minutes: None,
    };
    for line in lines {
        if battery.present
            && matches!(
                line.trim(),
                "Battery Warning: Early" | "Battery Warning: Final"
            )
        {
            continue;
        }
        if battery.present {
            return Err("multiple battery rows are unsupported");
        }
        let (prefix, rest) = line.split_once('%').ok_or("unrecognized battery row")?;
        let charge = prefix
            .split_whitespace()
            .last()
            .ok_or("missing battery charge")?
            .parse::<u32>()
            .map_err(|_| "invalid battery charge")?;
        if charge > 100 {
            return Err("battery charge exceeds 100 percent");
        }
        let mut parts = rest.split(';').map(str::trim);
        if parts.next() != Some("") {
            return Err("invalid battery charge separator");
        }
        let state = parts.next().ok_or("missing charging state")?;
        let charging = match state {
            "charging" | "finishing charge" => true,
            "discharging" | "charged" | "not charging" | "AC attached" => false,
            _ => return Err("unknown battery charging state"),
        };
        let tail = parts.next().ok_or("missing battery time")?;
        let state = if state == "AC attached" {
            if !tail.starts_with("not charging ") {
                return Err("unknown AC-attached state");
            }
            "AC attached; not charging"
        } else {
            state
        };
        let time = if tail.starts_with("(no estimate)") || state == "AC attached; not charging" {
            None
        } else {
            let clock = tail
                .split_whitespace()
                .next()
                .ok_or("missing battery time")?;
            let (h, m) = clock.split_once(':').ok_or("invalid battery time")?;
            let hours = h.parse::<u32>().map_err(|_| "invalid battery hours")?;
            let minutes = m.parse::<u32>().map_err(|_| "invalid battery minutes")?;
            if minutes >= 60 {
                return Err("invalid battery minutes");
            }
            Some(
                hours
                    .checked_mul(60)
                    .and_then(|v| v.checked_add(minutes))
                    .ok_or("battery time overflow")?,
            )
        };
        if !tail.contains("present: true") {
            return Err("battery presence is not confirmed");
        }
        battery.present = true;
        battery.charge_percent = Some(charge);
        battery.charging = Some(charging);
        battery.state = Some(state.into());
        battery.time_remaining_minutes = time;
    }
    Ok(battery)
}

pub struct Thermal {
    pub cpu_speed_limit_percent: Option<u32>,
    pub cpu_scheduler_limit_percent: Option<u32>,
    pub available_cpus: Option<u32>,
}

/// No recorded CPU power status means no recorded limit, rather than a measured 100%.
pub fn parse_thermal(text: &str) -> Result<Thermal, &'static str> {
    let mut thermal = Thermal {
        cpu_speed_limit_percent: None,
        cpu_scheduler_limit_percent: None,
        available_cpus: None,
    };
    for line in text.lines() {
        if let Some((key, value)) = line.trim().split_once(" = ") {
            let target = match key {
                "CPU_Speed_Limit" => &mut thermal.cpu_speed_limit_percent,
                "CPU_Scheduler_Limit" => &mut thermal.cpu_scheduler_limit_percent,
                "CPU_Available_CPUs" => &mut thermal.available_cpus,
                _ => continue,
            };
            let n = value
                .trim()
                .parse::<u32>()
                .map_err(|_| "invalid thermal number")?;
            if (key != "CPU_Available_CPUs" && n > 100) || target.is_some() {
                return Err("invalid or duplicate CPU limit");
            }
            *target = Some(n);
        }
    }
    if thermal.cpu_speed_limit_percent.is_none()
        && !text.contains("No CPU power status has been recorded")
    {
        return Err("no recognized CPU speed status");
    }
    Ok(thermal)
}

pub struct GpuTimes {
    pub window_seconds: f64,
    pub milliseconds: BTreeMap<u32, f64>,
}

/// Reads powermetrics' task GPU ms/s and its reported elapsed interval, preserving names with spaces.
pub fn parse_gpu_times(text: &str) -> Result<GpuTimes, &'static str> {
    let mut seconds = None;
    let mut columns = None;
    let mut milliseconds = BTreeMap::new();
    let mut finished = false;
    for line in text.lines() {
        if line.starts_with("*** Sampled system activity") {
            if seconds.is_some() {
                return Err("multiple GPU sampling windows");
            }
            let prefix = line
                .split_once("ms elapsed)")
                .ok_or("missing GPU sample duration")?
                .0;
            let duration = prefix
                .rsplit_once('(')
                .ok_or("invalid GPU sample duration")?
                .1;
            let value = nonnegative(duration)? / 1000.0;
            if value <= 0.0 {
                return Err("zero GPU sample duration");
            }
            seconds = Some(value);
        }
        let normalized = without_annotations(line)
            .replace("CPU ms/s", "CPU_ms/s")
            .replace("GPU ms/s", "GPU_ms/s");
        let fields: Vec<_> = normalized.split_whitespace().collect();
        if fields.first() == Some(&"Name") && fields.get(1) == Some(&"ID") {
            if columns.is_some() {
                return Err("multiple GPU task tables");
            }
            let gpu = fields
                .iter()
                .position(|f| *f == "GPU_ms/s")
                .ok_or("GPU column is absent")?;
            columns = Some((fields.len(), gpu));
        } else if let Some((count, gpu)) = columns {
            if line.trim().is_empty() || line.starts_with("***") {
                finished = true;
            }
            if finished {
                continue;
            }
            if fields.len() < count {
                return Err("truncated GPU task row");
            }
            let offset = fields.len() - count;
            let pid = fields
                .get(offset.checked_add(1).ok_or("GPU task column overflow")?)
                .ok_or("missing GPU task ID")?
                .parse::<u32>()
                .map_err(|_| "invalid GPU task ID")?;
            let rate = nonnegative(
                fields
                    .get(offset.checked_add(gpu).ok_or("GPU column overflow")?)
                    .ok_or("missing GPU task time")?,
            )?;
            let duration = seconds.ok_or("missing GPU window")?;
            let time = rate * duration;
            if !time.is_finite() || milliseconds.insert(pid, time).is_some() {
                return Err("invalid or duplicate GPU task time");
            }
        }
    }
    if columns.is_none() {
        return Err("GPU task table is absent");
    }
    Ok(GpuTimes {
        window_seconds: seconds.ok_or("GPU window is absent")?,
        milliseconds,
    })
}
fn without_annotations(text: &str) -> String {
    let mut result = String::new();
    let mut annotation = false;
    for c in text.chars() {
        if c == '(' {
            annotation = true;
        } else if c == ')' {
            annotation = false;
        } else if !annotation {
            result.push(c);
        }
    }
    result
}
