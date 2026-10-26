//! App accounting sums only processes macOS attributes to the same responsible process.
use std::collections::BTreeMap;

pub struct ProcessUsage {
    pub energy_impact: Option<f64>,
    pub app_pid: Option<u32>,
    pub cpu_percent: Option<f64>,
    pub resident_bytes: u64,
    pub footprint_bytes: Option<u64>,
    pub peak_footprint_bytes: Option<u64>,
    pub bytes_in_per_second: Option<f64>,
    pub bytes_out_per_second: Option<f64>,
    pub disk_read_bytes: Option<u64>,
    pub disk_written_bytes: Option<u64>,
}
pub struct AppUsage {
    pub energy_impact: Option<f64>,
    pub app_pid: u32,
    pub process_count: usize,
    pub cpu_percent: Option<f64>,
    pub resident_bytes: Option<u64>,
    pub footprint_bytes: Option<u64>,
    pub peak_footprint_bytes: Option<u64>,
    pub bytes_in_per_second: Option<f64>,
    pub bytes_out_per_second: Option<f64>,
    pub disk_read_bytes: Option<u64>,
    pub disk_written_bytes: Option<u64>,
}

/// Unknown attribution is excluded; unknown measurements propagate into their App total.
pub fn summarize(processes: &[ProcessUsage]) -> Vec<AppUsage> {
    let mut apps = BTreeMap::new();
    for p in processes {
        if let Some(pid) = p.app_pid {
            let app = apps.entry(pid).or_insert(AppUsage {
                energy_impact: Some(0.0),
                app_pid: pid,
                process_count: 0,
                cpu_percent: Some(0.0),
                resident_bytes: Some(0),
                footprint_bytes: Some(0),
                peak_footprint_bytes: Some(0),
                bytes_in_per_second: Some(0.0),
                bytes_out_per_second: Some(0.0),
                disk_read_bytes: Some(0),
                disk_written_bytes: Some(0),
            });
            app.process_count = app.process_count.saturating_add(1);
            app.disk_read_bytes = super::add_bytes(app.disk_read_bytes, p.disk_read_bytes);
            app.disk_written_bytes = super::add_bytes(app.disk_written_bytes, p.disk_written_bytes);
            app.cpu_percent = super::add_rates(app.cpu_percent, p.cpu_percent);
            app.energy_impact = super::add_rates(app.energy_impact, p.energy_impact);
            app.bytes_in_per_second =
                super::add_rates(app.bytes_in_per_second, p.bytes_in_per_second);
            app.bytes_out_per_second =
                super::add_rates(app.bytes_out_per_second, p.bytes_out_per_second);
            app.resident_bytes = super::add_bytes(app.resident_bytes, Some(p.resident_bytes));
            app.footprint_bytes = super::add_bytes(app.footprint_bytes, p.footprint_bytes);
            app.peak_footprint_bytes =
                super::add_bytes(app.peak_footprint_bytes, p.peak_footprint_bytes);
        }
    }
    apps.into_values().collect()
}
