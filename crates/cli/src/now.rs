//! One observation window, retaining missing measurements rather than manufacturing zeroes.
use crate::{
    direct::{self, Reading, Source},
    tools::ps,
};
use hostvitals_policy as policy;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    num::{NonZeroU64, NonZeroUsize},
    time::{Duration, Instant},
};

#[derive(Serialize)]
struct Process {
    pid: u32,
    parent_pid: u32,
    uid: u32,
    user: String,
    name: String,
    started_at: String,
    cpu_percent: Option<f64>,
    resident_bytes: u64,
    footprint_bytes: Option<u64>,
    peak_footprint_bytes: Option<u64>,
    app_pid: Option<u32>,
}
#[derive(Serialize)]
struct App {
    app_pid: u32,
    name: Option<String>,
    started_at: Option<String>,
    process_count: usize,
    cpu_percent: Option<f64>,
    resident_bytes: Option<u64>,
    footprint_bytes: Option<u64>,
    peak_footprint_bytes: Option<u64>,
}

pub fn answer(source: &Source, seconds: NonZeroU64, top: NonZeroUsize) -> Result<Value, String> {
    let before = ps::snapshot()?;
    let start = Instant::now();
    let host_before = host(source, 0)?;
    std::thread::sleep(Duration::from_secs(seconds.get()));
    let host_after = host(source, 1)?;
    let after = ps::snapshot()?;
    let window = start.elapsed().as_secs_f64();
    let mut skipped = Vec::new();
    let mut processes = Vec::new();
    let mut foreign = 0;
    let mut missing_cpu = 0;
    let mut usage_errors = BTreeMap::<String, usize>::new();
    let mut app_errors = BTreeMap::<String, usize>::new();
    for p in after.values() {
        let cpu_percent = before
            .get(&p.pid)
            .and_then(|old| {
                policy::cpu_delta(
                    old.cpu_hundredths,
                    p.cpu_hundredths,
                    old.started_at == p.started_at,
                )
            })
            .map(|delta| policy::cpu_percent(delta, window));
        if cpu_percent.is_none() {
            missing_cpu += 1;
        }
        let (footprint_bytes, peak_footprint_bytes) =
            if policy::may_read_footprint(host_after.euid, p.uid) {
                match source.read(&format!("now-rusage-{}", p.pid), || {
                    direct::rusage::read_pid(p.pid)
                }) {
                    Reading::Available { data } => {
                        (Some(data.footprint_bytes), Some(data.peak_footprint_bytes))
                    }
                    Reading::Unavailable { reason } => {
                        *usage_errors.entry(reason).or_default() += 1;
                        (None, None)
                    }
                }
            } else {
                foreign += 1;
                (None, None)
            };
        let app_pid = match source.read(&format!("now-responsibility-{}", p.pid), || {
            direct::responsibility::read_pid(p.pid)
        }) {
            Reading::Available { data } if data.pid == p.pid && data.app_pid > 0 => {
                Some(data.app_pid)
            }
            Reading::Available { .. } => {
                *app_errors
                    .entry("responsibility returned an invalid identity".into())
                    .or_default() += 1;
                None
            }
            Reading::Unavailable { reason } => {
                *app_errors.entry(reason).or_default() += 1;
                None
            }
        };
        processes.push(Process {
            pid: p.pid,
            parent_pid: p.parent_pid,
            uid: p.uid,
            user: p.user.clone(),
            name: p.name.clone(),
            started_at: p.started_at.clone(),
            cpu_percent,
            resident_bytes: p.resident_bytes,
            footprint_bytes,
            peak_footprint_bytes,
            app_pid,
        });
    }
    if foreign > 0 {
        skipped.push(json!({"measurement":"footprint","count":foreign,"reason":"other users' footprint and lifetime peak require root; CPU and resident memory are included","fix":"sudo hostvitals now"}));
    }
    if missing_cpu > 0 {
        skipped.push(json!({"measurement":"cpu_percent","count":missing_cpu,"reason":"no matching initial PID and start time, or cumulative CPU counter regressed","fix":"hostvitals now"}));
    }
    for (reason, count) in usage_errors {
        skipped.push(json!({"measurement":"footprint","count":count,"reason":reason,"fix":"hostvitals doctor && hostvitals now"}));
    }
    for (reason, count) in app_errors {
        skipped.push(json!({"measurement":"app_pid","count":count,"reason":reason,"fix":"brew upgrade sunfmin/tap/hostvitals && hostvitals doctor"}));
    }
    let usages: Vec<_> = processes
        .iter()
        .map(|p| policy::apps::ProcessUsage {
            app_pid: p.app_pid,
            cpu_percent: p.cpu_percent,
            resident_bytes: p.resident_bytes,
            footprint_bytes: p.footprint_bytes,
            peak_footprint_bytes: p.peak_footprint_bytes,
        })
        .collect();
    let apps: Vec<_> = policy::apps::summarize(&usages)
        .into_iter()
        .map(|app| {
            let identity = after.get(&app.app_pid);
            App {
                app_pid: app.app_pid,
                name: identity.map(|p| p.name.clone()),
                started_at: identity.map(|p| p.started_at.clone()),
                process_count: app.process_count,
                cpu_percent: app.cpu_percent,
                resident_bytes: app.resident_bytes,
                footprint_bytes: app.footprint_bytes,
                peak_footprint_bytes: app.peak_footprint_bytes,
            }
        })
        .collect();
    let mut process_cpu: Vec<_> = processes
        .iter()
        .filter(|p| p.cpu_percent.is_some())
        .collect();
    process_cpu.sort_by(|a, b| {
        policy::rank_order(a.cpu_percent.unwrap(), a.pid, b.cpu_percent.unwrap(), b.pid)
    });
    let mut process_memory: Vec<_> = processes.iter().collect();
    process_memory.sort_by(|a, b| {
        b.resident_bytes
            .cmp(&a.resident_bytes)
            .then(a.pid.cmp(&b.pid))
    });
    let mut app_cpu: Vec<_> = apps.iter().filter(|p| p.cpu_percent.is_some()).collect();
    app_cpu.sort_by(|a, b| {
        policy::rank_order(
            a.cpu_percent.unwrap(),
            a.app_pid,
            b.cpu_percent.unwrap(),
            b.app_pid,
        )
    });
    let mut app_memory: Vec<_> = apps.iter().filter(|p| p.resident_bytes.is_some()).collect();
    app_memory.sort_by(|a, b| {
        b.resident_bytes
            .cmp(&a.resident_bytes)
            .then(a.app_pid.cmp(&b.app_pid))
    });
    process_cpu.truncate(top.get());
    process_memory.truncate(top.get());
    app_cpu.truncate(top.get());
    app_memory.truncate(top.get());
    let ticks: Vec<u64> = host_before
        .cpu_ticks
        .into_iter()
        .zip(host_after.cpu_ticks)
        .map(|(b, a)| policy::tick_delta(b, a))
        .collect();
    let total = ticks.iter().sum::<u64>() as f64;
    if total == 0.0 {
        return Err("host CPU ticks did not advance; run hostvitals now --seconds 3".into());
    }
    Ok(
        json!({"source":if source.0.is_some(){"fixtures"}else{"host"},"window_seconds":window,
        "host":{"cpu":{"user_percent":(ticks[0]+ticks[3]) as f64/total*100.0,"system_percent":ticks[1] as f64/total*100.0,"idle_percent":ticks[2] as f64/total*100.0,"logical_cores":host_after.logical_cores,"physical_cores":host_after.physical_cores},"load_average":host_after.load_average,"memory":host_after.memory},
        "processes":processes,"apps":apps,"rankings":{"processes_by_cpu":process_cpu,"processes_by_memory":process_memory,"apps_by_cpu":app_cpu,"apps_by_memory":app_memory},"skipped":skipped}),
    )
}
fn host(source: &Source, index: u8) -> Result<direct::host::Snapshot, String> {
    match source.read(&format!("now-host-{index}"), direct::host::read) {
        Reading::Available { data } => Ok(data),
        Reading::Unavailable { reason } => Err(format!(
            "host snapshot: {reason}; run hostvitals doctor && hostvitals now"
        )),
    }
}
