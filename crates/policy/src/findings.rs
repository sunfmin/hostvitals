//! Fixed judgements over complete observations, independent of collection and rankings.
use flux_rs::attrs::*;

pub const SWAP_USED_PERCENT: u64 = 80;
pub const PROCESS_RESIDENT_PERCENT: u64 = 10;
pub const PROCESS_CPU_CORES: u128 = 1;
const NANOS_PER_HUNDREDTH: u128 = 10_000_000;
pub const CPU_SPEED_LIMIT_PERCENT: u32 = 100;

/// Only an observed speed limit below the full-speed threshold is a throttling finding.
#[spec(fn(limit: u32) -> bool[limit < CPU_SPEED_LIMIT_PERCENT])]
pub fn cpu_throttled(limit: u32) -> bool {
    limit < CPU_SPEED_LIMIT_PERCENT
}

#[spec(fn() -> Pressure[Pressure::Warn])]
pub fn memory_pressure_threshold() -> Pressure {
    Pressure::Warn
}

#[reflect]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pressure {
    Normal,
    Warn,
    Critical,
}
impl Pressure {
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Warn => "warn",
            Self::Critical => "critical",
        }
    }

    #[spec(fn(self: Pressure) -> u8[if self == Pressure::Normal { 0 } else if self == Pressure::Warn { 1 } else { 2 }])]
    fn level(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Warn => 1,
            Self::Critical => 2,
        }
    }
}

pub struct Host {
    pub pressure: Pressure,
    pub memory_bytes: u64,
    pub swap_used_bytes: u64,
    pub swap_total_bytes: u64,
}

pub struct Process {
    pub pid: u32,
    pub resident_bytes: u64,
    /// None means no matching baseline; it cannot produce a CPU finding.
    pub cpu_hundredths: Option<u64>,
}

pub enum Finding {
    MemoryPressure {
        pressure: Pressure,
    },
    SwapUsed {
        used_bytes: u64,
        total_bytes: u64,
    },
    ProcessResident {
        pid: u32,
        resident_bytes: u64,
        total_bytes: u64,
    },
    ProcessCpu {
        pid: u32,
        cpu_hundredths: u64,
        window_nanoseconds: u128,
    },
}

/// Judges every supplied Process, regardless of how many records a caller ranks.
/// A zero denominator never yields a ratio finding.
pub fn judge(host: &Host, processes: &[Process], window_nanoseconds: u128) -> Vec<Finding> {
    let mut findings = Vec::new();
    if memory_pressure(host.pressure) {
        findings.push(Finding::MemoryPressure {
            pressure: host.pressure,
        });
    }
    if past_percent(
        host.swap_used_bytes,
        host.swap_total_bytes,
        SWAP_USED_PERCENT,
    ) {
        findings.push(Finding::SwapUsed {
            used_bytes: host.swap_used_bytes,
            total_bytes: host.swap_total_bytes,
        });
    }
    for p in processes {
        if past_percent(
            p.resident_bytes,
            host.memory_bytes,
            PROCESS_RESIDENT_PERCENT,
        ) {
            findings.push(Finding::ProcessResident {
                pid: p.pid,
                resident_bytes: p.resident_bytes,
                total_bytes: host.memory_bytes,
            });
        }
        if let Some(cpu_hundredths) = p.cpu_hundredths
            && sustained_cpu(cpu_hundredths, window_nanoseconds)
        {
            findings.push(Finding::ProcessCpu {
                pid: p.pid,
                cpu_hundredths,
                window_nanoseconds,
            });
        }
    }
    findings
}

#[spec(fn(pressure: Pressure) -> bool[pressure != Pressure::Normal])]
fn memory_pressure(pressure: Pressure) -> bool {
    pressure.level() >= memory_pressure_threshold().level()
}

#[spec(fn(value: u64, total: u64, percent: u64{percent <= 100}) -> bool[total > 0 && value * 100 > total * percent])]
fn past_percent(value: u64, total: u64, percent: u64) -> bool {
    total > 0 && value as u128 * 100 > total as u128 * percent as u128
}

#[spec(fn(hundredths: u64, window: u128) -> bool[window > 0 && hundredths * NANOS_PER_HUNDREDTH >= window * PROCESS_CPU_CORES])]
fn sustained_cpu(hundredths: u64, window: u128) -> bool {
    window > 0 && hundredths as u128 * NANOS_PER_HUNDREDTH >= window * PROCESS_CPU_CORES
}

#[cfg(flux)]
#[allow(dead_code)]
mod rejected {
    use super::*;

    #[should_fail]
    fn full_speed_is_throttled() {
        flux_rs::assert(cpu_throttled(100));
    }

    #[should_fail]
    fn normal_pressure_is_a_finding() {
        flux_rs::assert(memory_pressure(Pressure::Normal));
    }

    #[should_fail]
    fn swap_at_threshold_is_a_finding() {
        flux_rs::assert(past_percent(800, 1_000, SWAP_USED_PERCENT));
    }

    #[should_fail]
    fn resident_at_threshold_is_a_finding() {
        flux_rs::assert(past_percent(100, 1_000, PROCESS_RESIDENT_PERCENT));
    }

    #[should_fail]
    fn cpu_below_one_core_is_a_finding() {
        flux_rs::assert(sustained_cpu(99, 1_000_000_000));
    }
}
