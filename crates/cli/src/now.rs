//! One observation window, retaining missing measurements rather than manufacturing zeroes.
use crate::{
    direct::{self, Reading, Source},
    tools::{nettop, ps},
};
use hostvitals_policy as policy;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
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
    network: Network,
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
    network: Network,
}

#[derive(Serialize)]
struct Network {
    bytes_in_per_second: Option<f64>,
    bytes_out_per_second: Option<f64>,
}
impl Network {
    fn new(change: &policy::network::Change, seconds: f64) -> Self {
        Self {
            bytes_in_per_second: policy::network::per_second(change.bytes_in, seconds),
            bytes_out_per_second: policy::network::per_second(change.bytes_out, seconds),
        }
    }
}

pub fn answer(source: &Source, seconds: NonZeroU64, top: NonZeroUsize) -> Result<Value, String> {
    let before = ps::snapshot()?;
    let start = Instant::now();
    let host_before = host(source, 0)?;
    let network_before = nettop::snapshot();
    let network_start = Instant::now();
    std::thread::sleep(Duration::from_secs(seconds.get()));
    let network_after = nettop::snapshot();
    let network_seconds = network_start.elapsed().as_secs_f64();
    let host_after = host(source, 1)?;
    let after = ps::snapshot()?;
    let window = start.elapsed().as_secs_f64();
    let mut skipped = Vec::new();
    let mut processes = Vec::new();
    let mut foreign = 0;
    let mut missing_cpu = 0;
    let mut usage_errors = BTreeMap::<String, usize>::new();
    let mut app_errors = BTreeMap::<String, usize>::new();
    let mut network_changes = BTreeMap::new();
    let mut host_network = policy::network::Change {
        bytes_in: None,
        bytes_out: None,
    };
    match (&network_before, &network_after) {
        (Ok(old), Ok(new)) => {
            let identities = after
                .values()
                .map(|p| {
                    (
                        p.pid,
                        before
                            .get(&p.pid)
                            .is_some_and(|old| old.started_at == p.started_at),
                    )
                })
                .collect();
            let changes = policy::network::changes(old, new, &identities);
            host_network = changes.host;
            network_changes = changes.processes;
        }
        _ => {
            let reasons: BTreeSet<_> =
                [network_before.as_ref().err(), network_after.as_ref().err()]
                    .into_iter()
                    .flatten()
                    .collect();
            skipped.push(json!({"measurement":"network","count":after.len()+1,"reason":reasons.into_iter().cloned().collect::<Vec<_>>().join("; "),"fix":"PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH hostvitals doctor && hostvitals now"}));
        }
    }
    for p in after.values() {
        let cpu_percent = before
            .get(&p.pid)
            .and_then(|old| {
                policy::counter_delta(
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
            network: Network::new(
                network_changes
                    .get(&p.pid)
                    .unwrap_or(&policy::network::Change {
                        bytes_in: None,
                        bytes_out: None,
                    }),
                network_seconds,
            ),
        });
    }
    if network_before.is_ok() && network_after.is_ok() {
        for (field, count, host_missing) in [
            (
                "bytes_in_per_second",
                processes
                    .iter()
                    .filter(|p| p.network.bytes_in_per_second.is_none())
                    .count(),
                host_network.bytes_in.is_none(),
            ),
            (
                "bytes_out_per_second",
                processes
                    .iter()
                    .filter(|p| p.network.bytes_out_per_second.is_none())
                    .count(),
                host_network.bytes_out.is_none(),
            ),
        ] {
            if count > 0 {
                skipped.push(json!({"measurement":format!("network.{field}"),"count":count,"reason":"no matching Process across ps readings, only one nettop reading, or byte counter regressed","fix":"hostvitals now --seconds 3"}));
            }
            if host_missing {
                skipped.push(json!({"measurement":format!("host.network.{field}"),"count":1,"reason":"incomplete process byte changes or total overflow; a partial sum is not host throughput","fix":"hostvitals now --seconds 3"}));
            }
        }
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
            bytes_in_per_second: p.network.bytes_in_per_second,
            bytes_out_per_second: p.network.bytes_out_per_second,
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
                network: Network {
                    bytes_in_per_second: app.bytes_in_per_second,
                    bytes_out_per_second: app.bytes_out_per_second,
                },
            }
        })
        .collect();
    let process_cpu = ranked(&processes, top.get(), |p| p.cpu_percent, |p| p.pid);
    let process_in = ranked(
        &processes,
        top.get(),
        |p| p.network.bytes_in_per_second,
        |p| p.pid,
    );
    let process_out = ranked(
        &processes,
        top.get(),
        |p| p.network.bytes_out_per_second,
        |p| p.pid,
    );
    let app_in = ranked(
        &apps,
        top.get(),
        |p| p.network.bytes_in_per_second,
        |p| p.app_pid,
    );
    let app_out = ranked(
        &apps,
        top.get(),
        |p| p.network.bytes_out_per_second,
        |p| p.app_pid,
    );
    let mut process_memory: Vec<_> = processes.iter().collect();
    process_memory.sort_by(|a, b| {
        b.resident_bytes
            .cmp(&a.resident_bytes)
            .then(a.pid.cmp(&b.pid))
    });
    let app_cpu = ranked(&apps, top.get(), |p| p.cpu_percent, |p| p.app_pid);
    let mut app_memory: Vec<_> = apps.iter().filter(|p| p.resident_bytes.is_some()).collect();
    app_memory.sort_by(|a, b| {
        b.resident_bytes
            .cmp(&a.resident_bytes)
            .then(a.app_pid.cmp(&b.app_pid))
    });
    process_memory.truncate(top.get());
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
    Ok(json!({
        "source": if source.0.is_some() { "fixtures" } else { "host" },
        "window_seconds": window,
        "host": {
            "cpu": {
                "user_percent": (ticks[0] + ticks[3]) as f64 / total * 100.0,
                "system_percent": ticks[1] as f64 / total * 100.0,
                "idle_percent": ticks[2] as f64 / total * 100.0,
                "logical_cores": host_after.logical_cores,
                "physical_cores": host_after.physical_cores
            },
            "load_average": host_after.load_average,
            "memory": host_after.memory,
            "network": {
                "window_seconds": if network_before.is_ok() && network_after.is_ok() { Some(network_seconds) } else { None },
                "bytes_in_per_second": policy::network::per_second(host_network.bytes_in, network_seconds),
                "bytes_out_per_second": policy::network::per_second(host_network.bytes_out, network_seconds)
            }
        },
        "processes": processes,
        "apps": apps,
        "rankings": {
            "processes_by_cpu": process_cpu,
            "processes_by_memory": process_memory,
            "apps_by_cpu": app_cpu,
            "apps_by_memory": app_memory,
            "processes_by_network_in": process_in,
            "processes_by_network_out": process_out,
            "apps_by_network_in": app_in,
            "apps_by_network_out": app_out
        },
        "skipped": skipped
    }))
}

fn ranked<T>(
    items: &[T],
    top: usize,
    value: impl Fn(&T) -> Option<f64>,
    pid: impl Fn(&T) -> u32,
) -> Vec<&T> {
    let mut measured: Vec<_> = items
        .iter()
        .filter_map(|item| value(item).map(|value| (item, value)))
        .collect();
    measured.sort_by(|(a, a_value), (b, b_value)| {
        policy::rank_order(*a_value, pid(a), *b_value, pid(b))
    });
    measured
        .into_iter()
        .take(top)
        .map(|(item, _)| item)
        .collect()
}
fn host(source: &Source, index: u8) -> Result<direct::host::Snapshot, String> {
    match source.read(&format!("now-host-{index}"), direct::host::read) {
        Reading::Available { data } => Ok(data),
        Reading::Unavailable { reason } => Err(format!(
            "host snapshot: {reason}; run hostvitals doctor && hostvitals now"
        )),
    }
}
