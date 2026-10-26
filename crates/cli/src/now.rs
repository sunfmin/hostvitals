//! One observation window, retaining missing measurements rather than manufacturing zeroes.
use crate::{
    direct::{self, Reading, Source},
    tools::{df, ioreg, nettop, ps, top as top_tool},
};
use hostvitals_policy as policy;
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU64, NonZeroUsize},
    time::{Duration, Instant},
};

// Struct field order keeps judgements before the raw sections, even though Value sorts keys.
#[derive(Serialize)]
pub(crate) struct Answer {
    findings: Vec<Value>,
    #[serde(flatten)]
    measurements: Value,
}

#[derive(Serialize)]
struct Process {
    energy_impact: Option<f64>,
    gpu_time_ms: Option<f64>,
    pid: u32,
    parent_pid: u32,
    uid: u32,
    user: String,
    name: String,
    started_at: String,
    cpu_percent: Option<f64>,
    #[serde(skip)]
    cpu_hundredths: Option<u64>,
    resident_bytes: u64,
    footprint_bytes: Option<u64>,
    peak_footprint_bytes: Option<u64>,
    app_pid: Option<u32>,
    network: Network,
    disk: ProcessDisk,
}
#[derive(Serialize)]
struct App {
    energy_impact: Option<f64>,
    app_pid: u32,
    name: Option<String>,
    started_at: Option<String>,
    process_count: usize,
    cpu_percent: Option<f64>,
    resident_bytes: Option<u64>,
    footprint_bytes: Option<u64>,
    peak_footprint_bytes: Option<u64>,
    network: Network,
    disk: Disk,
}

#[derive(Serialize)]
struct Disk {
    read_bytes: Option<u64>,
    written_bytes: Option<u64>,
}

#[derive(Serialize)]
struct ProcessDisk {
    read_bytes: Option<u64>,
    written_bytes: Option<u64>,
    window_seconds: Option<f64>,
}

struct Usage {
    reading: Reading<direct::rusage::Counters>,
    at: Instant,
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

pub fn answer(source: &Source, seconds: NonZeroU64, top: NonZeroUsize) -> Result<Answer, String> {
    std::thread::scope(|scope| {
        let before = ps::snapshot()?;
        let start = Instant::now();
        let host_before = host(source, 0)?;
        let usage_before = usages(source, &before, host_before.euid, true);
        let energy_worker = scope.spawn(move || top_tool::energy(seconds.get()));
        let gpu_worker = if host_before.euid == 0 {
            Some(scope.spawn(move || crate::tools::powermetrics::gpu_times(seconds.get())))
        } else {
            None
        };
        let power_window = if source.0.is_none() {
            Some(direct::ioreport::Window::start(true))
        } else {
            None
        };
        let network_before = nettop::snapshot();
        let network_start = Instant::now();
        let disk_before = ioreg::snapshot();
        let disk_start = Instant::now();
        std::thread::sleep(Duration::from_secs(seconds.get()));
        let disk_after = ioreg::snapshot();
        let disk_seconds = disk_start.elapsed().as_secs_f64();
        let network_after = nettop::snapshot();
        let network_seconds = network_start.elapsed().as_secs_f64();
        let host_after = host(source, 1)?;
        let power_reading = source.read("now-power", || match power_window {
            Some(Ok(window)) => window.finish(),
            Some(Err(reason)) => Reading::Unavailable { reason },
            None => unreachable!("fixtures never invoke the native reader"),
        });
        let after = ps::snapshot()?;
        let elapsed = start.elapsed();
        let window = elapsed.as_secs_f64();
        let usage_after = usages(source, &after, host_after.euid, false);
        let mut energy_skipped = Vec::new();
        let gpu = match gpu_worker {
            Some(worker) => match worker
                .join()
                .unwrap_or_else(|_| Err("GPU reader failed".into()))
            {
                Ok(times) => Some(times),
                Err(reason) => {
                    energy_skipped.push(json!({"measurement":"gpu_time_ms","count":after.len(),"reason":reason,"fix":format!("PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH hostvitals now --seconds {} --top {}",seconds,top)}));
                    None
                }
            },
            None => {
                energy_skipped.push(json!({"measurement":"gpu_time_ms","count":after.len(),"reason":"per-process GPU time requires root (powermetrics --samplers tasks --show-process-gpu)","fix":format!("sudo hostvitals now --seconds {} --top {}",seconds,top)}));
                None
            }
        };
        let power = crate::energy::power(power_reading, &mut energy_skipped);
        let scores = match energy_worker
            .join()
            .unwrap_or_else(|_| Err("top reader failed".into()))
        {
            Ok(scores) => scores,
            Err(reason) => {
                energy_skipped.push(json!({"measurement":"energy_impact","count":after.len(),"reason":reason,"fix":"PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH hostvitals doctor && hostvitals now"}));
                BTreeMap::new()
            }
        };
        let mut skipped = Vec::new();
        let disk_reading = match (&disk_before, &disk_after) {
            (Ok(before), Ok(after)) => {
                policy::disk::host_change(before, after).map_err(str::to_owned)
            }
            _ => Err([disk_before.as_ref().err(), disk_after.as_ref().err()]
                .into_iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join("; ")),
        };
        let host_disk = match disk_reading {
            Ok(change) => {
                for (field, missing) in [
                    ("read_bytes_per_second", change.read_bytes.is_none()),
                    ("written_bytes_per_second", change.written_bytes.is_none()),
                ] {
                    if missing {
                        skipped.push(json!({"measurement":format!("host.disk.{field}"),"count":1,"reason":"missing or invalid byte counter, regression, or checked total overflow","fix":"hostvitals doctor && hostvitals now --seconds 3"}));
                    }
                }
                change
            }
            Err(reason) => {
                skipped.push(json!({"measurement":"host.disk","count":1,"reason":reason,"fix":"PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH hostvitals doctor && hostvitals now"}));
                policy::disk::Change {
                    read_bytes: None,
                    written_bytes: None,
                }
            }
        };
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
            let cpu_hundredths = before.get(&p.pid).and_then(|old| {
                policy::counter_delta(
                    old.cpu_hundredths,
                    p.cpu_hundredths,
                    old.started_at == p.started_at,
                )
            });
            let cpu_percent = cpu_hundredths.map(|delta| policy::cpu_percent(delta, window));
            if cpu_percent.is_none() {
                missing_cpu += 1;
            }
            let (footprint_bytes, peak_footprint_bytes) =
                if policy::may_read_footprint(host_after.euid, p.uid) {
                    match &usage_after[&p.pid].reading {
                        Reading::Available { data } => {
                            (Some(data.footprint_bytes), Some(data.peak_footprint_bytes))
                        }
                        Reading::Unavailable { reason } => {
                            *usage_errors.entry(reason.clone()).or_default() += 1;
                            (None, None)
                        }
                    }
                } else {
                    foreign += 1;
                    (None, None)
                };
            let same_process = before
                .get(&p.pid)
                .is_some_and(|old| old.started_at == p.started_at && old.uid == p.uid);
            let old = usage_before.get(&p.pid);
            let new = usage_after.get(&p.pid);
            let disk = policy::disk::process_change(
                usage_counters(old).as_ref(),
                usage_counters(new).as_ref(),
                same_process,
            );
            let disk_window_seconds =
                if same_process && usage_counters(old).is_some() && usage_counters(new).is_some() {
                    old.zip(new)
                        .map(|(old, new)| new.at.duration_since(old.at).as_secs_f64())
                } else {
                    None
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
                gpu_time_ms: before
                    .get(&p.pid)
                    .filter(|old| old.started_at == p.started_at)
                    .and_then(|_| gpu.as_ref()?.milliseconds.get(&p.pid).copied()),
                energy_impact: before
                    .get(&p.pid)
                    .filter(|old| old.started_at == p.started_at)
                    .and_then(|_| scores.get(&p.pid).copied()),
                pid: p.pid,
                parent_pid: p.parent_pid,
                uid: p.uid,
                user: p.user.clone(),
                name: p.name.clone(),
                started_at: p.started_at.clone(),
                cpu_percent,
                cpu_hundredths,
                resident_bytes: p.resident_bytes,
                footprint_bytes,
                peak_footprint_bytes,
                app_pid,
                disk: ProcessDisk {
                    read_bytes: disk.read_bytes,
                    written_bytes: disk.written_bytes,
                    window_seconds: disk_window_seconds,
                },
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
            skipped.push(json!({"measurement":"disk","count":foreign,"reason":"other users' rusage disk counters require root; CPU and resident memory are included","fix":"sudo hostvitals now"}));
        }
        for (field, missing) in [
            (
                "read_bytes",
                processes
                    .iter()
                    .filter(|p| {
                        policy::may_read_footprint(host_after.euid, p.uid)
                            && p.disk.read_bytes.is_none()
                    })
                    .count(),
            ),
            (
                "written_bytes",
                processes
                    .iter()
                    .filter(|p| {
                        policy::may_read_footprint(host_after.euid, p.uid)
                            && p.disk.written_bytes.is_none()
                    })
                    .count(),
            ),
        ] {
            if missing > 0 {
                let reasons: BTreeSet<_> = usage_before
                    .values()
                    .chain(usage_after.values())
                    .filter_map(|u| match &u.reading {
                        Reading::Unavailable { reason } => Some(reason.clone()),
                        _ => None,
                    })
                    .collect();
                skipped.push(json!({"measurement":format!("disk.{field}"),"count":missing,"reason":format!("no matching PID/start time and owner, missing rusage reading, or regressed counter; {}",reasons.into_iter().collect::<Vec<_>>().join("; ")),"fix":"hostvitals doctor && hostvitals now --seconds 3"}));
            }
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
        if !scores.is_empty() {
            let missing = processes
                .iter()
                .filter(|p| p.energy_impact.is_none())
                .count();
            if missing > 0 {
                energy_skipped.push(json!({"measurement":"energy_impact","count":missing,"reason":"no matching PID and start time across inventories, or Process absent from top's final sample","fix":"hostvitals now --seconds 3"}));
            }
        }
        if gpu.is_some() {
            let missing = processes.iter().filter(|p| p.gpu_time_ms.is_none()).count();
            if missing > 0 {
                energy_skipped.push(json!({"measurement":"gpu_time_ms","count":missing,"reason":"no matching PID and start time across inventories, or Process absent from powermetrics tasks","fix":format!("sudo hostvitals now --seconds {} --top {}",seconds,top)}));
            }
        }
        skipped.extend(energy_skipped);
        let pressure = match host_after.memory.pressure.as_str() {
            "normal" => policy::findings::Pressure::Normal,
            "warn" => policy::findings::Pressure::Warn,
            "critical" => policy::findings::Pressure::Critical,
            unknown => {
                return Err(format!(
                    "unknown memory pressure {unknown}; run hostvitals doctor && hostvitals now"
                ));
            }
        };
        let observations: Vec<_> = processes
            .iter()
            .map(|p| policy::findings::Process {
                pid: p.pid,
                resident_bytes: p.resident_bytes,
                cpu_hundredths: p.cpu_hundredths,
            })
            .collect();
        let judgements = policy::findings::judge(
            &policy::findings::Host {
                pressure,
                memory_bytes: host_after.memory.total_bytes,
                swap_used_bytes: host_after.memory.swap_used_bytes,
                swap_total_bytes: host_after.memory.swap_total_bytes,
            },
            &observations,
            elapsed.as_nanos(),
        );
        let process_subject = |pid| {
            let p = &after[&pid];
            json!({"kind":"process", "pid":p.pid, "started_at":p.started_at, "name":p.name})
        };
        let mut findings: Vec<Value> = judgements.into_iter().map(|finding| {
        use policy::findings::{self, Finding};
        match finding {
            Finding::MemoryPressure { pressure } => json!({
                "rule":"memory_pressure", "subject":{"kind":"host"},
                "measured":{"pressure":pressure.name()},
                "threshold":{"comparison":">=", "pressure":findings::memory_pressure_threshold().name()},
                "next_command":"hostvitals now --top 20"
            }),
            Finding::SwapUsed { used_bytes, total_bytes } => json!({
                "rule":"swap_used", "subject":{"kind":"host"},
                "measured":{"used_bytes":used_bytes, "total_bytes":total_bytes, "percent":used_bytes as f64 / total_bytes as f64 * 100.0},
                "threshold":{"comparison":">", "percent":findings::SWAP_USED_PERCENT},
                "next_command":"hostvitals now --top 20"
            }),
            Finding::ProcessResident { pid, resident_bytes, total_bytes } => json!({
                "rule":"process_resident", "subject":process_subject(pid),
                "measured":{"resident_bytes":resident_bytes, "host_memory_bytes":total_bytes, "percent":resident_bytes as f64 / total_bytes as f64 * 100.0},
                "threshold":{"comparison":">", "percent":findings::PROCESS_RESIDENT_PERCENT},
                "next_command":format!("hostvitals memory {pid}")
            }),
            Finding::ProcessCpu { pid, cpu_hundredths, window_nanoseconds } => json!({
                "rule":"process_cpu", "subject":process_subject(pid),
                "measured":{"cpu_hundredths":cpu_hundredths, "window_nanoseconds":window_nanoseconds, "window_seconds":window, "cpu_percent":policy::cpu_percent(cpu_hundredths, window)},
                "threshold":{"comparison":">=", "cores":findings::PROCESS_CPU_CORES},
                "next_command":format!("hostvitals profile {pid}")
            }),
        }
    }).collect();
        let volumes = match df::snapshot() {
            Ok(volumes) => {
                let unavailable = volumes.iter().filter(|v| v.free_share.is_none()).count();
                if unavailable > 0 {
                    skipped.push(json!({"measurement":"volumes","count":unavailable,"reason":"volume capacity or available bytes unknown, invalid, overflowed or zero capacity; no free-space ratio can be judged","fix":"df -a -l -k -P && hostvitals now"}));
                }
                for v in &volumes {
                    if policy::disk::low_free(v.size_bytes, v.free_bytes) {
                        findings.push(json!({"rule":"volume_free","subject":{"kind":"volume","filesystem":v.filesystem,"mount_point":v.mount_point},"measured":{"size_bytes":v.size_bytes,"free_bytes":v.free_bytes,"free_share":v.free_share},"threshold":{"comparison":"<","percent":policy::disk::VOLUME_FREE_PERCENT},"next_command":"diskreap scan"}));
                    }
                }
                Some(volumes)
            }
            Err(reason) => {
                skipped.push(json!({"measurement":"volumes","count":1,"reason":reason,"fix":"PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH df -a -l -k -P && hostvitals now"}));
                None
            }
        };
        let battery = crate::energy::battery(&mut skipped);
        let thermal = crate::energy::thermal(&mut skipped, &mut findings);
        let usages: Vec<_> = processes
            .iter()
            .map(|p| policy::apps::ProcessUsage {
                energy_impact: p.energy_impact,
                app_pid: p.app_pid,
                cpu_percent: p.cpu_percent,
                resident_bytes: p.resident_bytes,
                footprint_bytes: p.footprint_bytes,
                peak_footprint_bytes: p.peak_footprint_bytes,
                bytes_in_per_second: p.network.bytes_in_per_second,
                bytes_out_per_second: p.network.bytes_out_per_second,
                disk_read_bytes: p.disk.read_bytes,
                disk_written_bytes: p.disk.written_bytes,
            })
            .collect();
        let apps: Vec<_> = policy::apps::summarize(&usages)
            .into_iter()
            .map(|app| {
                let identity = after.get(&app.app_pid);
                App {
                    energy_impact: app.energy_impact,
                    app_pid: app.app_pid,
                    name: identity.map(|p| p.name.clone()),
                    started_at: identity.map(|p| p.started_at.clone()),
                    process_count: app.process_count,
                    cpu_percent: app.cpu_percent,
                    resident_bytes: app.resident_bytes,
                    footprint_bytes: app.footprint_bytes,
                    peak_footprint_bytes: app.peak_footprint_bytes,
                    disk: Disk {
                        read_bytes: app.disk_read_bytes,
                        written_bytes: app.disk_written_bytes,
                    },
                    network: Network {
                        bytes_in_per_second: app.bytes_in_per_second,
                        bytes_out_per_second: app.bytes_out_per_second,
                    },
                }
            })
            .collect();
        for (field, count) in [
            (
                "read_bytes",
                apps.iter().filter(|a| a.disk.read_bytes.is_none()).count(),
            ),
            (
                "written_bytes",
                apps.iter()
                    .filter(|a| a.disk.written_bytes.is_none())
                    .count(),
            ),
        ] {
            if count > 0 {
                skipped.push(json!({"measurement":format!("apps.disk.{field}"),"count":count,"reason":"a contributing Process was unmeasured or the checked App byte sum overflowed","fix":"sudo hostvitals now --seconds 3"}));
            }
        }
        let process_writes: Vec<_> = policy::disk::rank_written(
            &processes
                .iter()
                .map(|p| (p.pid, p.disk.written_bytes))
                .collect::<Vec<_>>(),
            top.get(),
        )
        .into_iter()
        .filter_map(|pid| processes.iter().find(|p| p.pid == pid))
        .collect();
        let app_writes: Vec<_> = policy::disk::rank_written(
            &apps
                .iter()
                .map(|a| (a.app_pid, a.disk.written_bytes))
                .collect::<Vec<_>>(),
            top.get(),
        )
        .into_iter()
        .filter_map(|pid| apps.iter().find(|a| a.app_pid == pid))
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
        Ok(Answer {
            findings,
            measurements: json!({
                "source": if source.0.is_some() { "fixtures" } else { "host" },
                "window_seconds": window,
                "host": {
                    "battery": battery,
                    "thermal": thermal,
                    "power": power,
                    "gpu_time_window_seconds": gpu.as_ref().map(|g| g.window_seconds),
                    "cpu": {
                        "user_percent": (ticks[0] + ticks[3]) as f64 / total * 100.0,
                        "system_percent": ticks[1] as f64 / total * 100.0,
                        "idle_percent": ticks[2] as f64 / total * 100.0,
                        "logical_cores": host_after.logical_cores,
                        "physical_cores": host_after.physical_cores
                    },
                    "load_average": host_after.load_average,
                    "disk": {
                        "window_seconds": if disk_before.is_ok() && disk_after.is_ok() { Some(disk_seconds) } else { None },
                        "read_bytes_per_second": policy::network::per_second(host_disk.read_bytes, disk_seconds),
                        "written_bytes_per_second": policy::network::per_second(host_disk.written_bytes, disk_seconds)
                    },
                    "memory": host_after.memory,
                    "network": {
                        "window_seconds": if network_before.is_ok() && network_after.is_ok() { Some(network_seconds) } else { None },
                        "bytes_in_per_second": policy::network::per_second(host_network.bytes_in, network_seconds),
                        "bytes_out_per_second": policy::network::per_second(host_network.bytes_out, network_seconds)
                    }
                },
                "processes": processes,
                "apps": apps,
                "volumes": volumes,
                "rankings": {
                    "processes_by_cpu": process_cpu,
                    "processes_by_memory": process_memory,
                    "apps_by_cpu": app_cpu,
                    "apps_by_memory": app_memory,
                    "processes_by_network_in": process_in,
                    "processes_by_network_out": process_out,
                    "apps_by_network_in": app_in,
                    "apps_by_network_out": app_out,
                    "processes_by_disk_written": process_writes,
                    "apps_by_disk_written": app_writes,
                    "processes_by_energy": ranked(&processes, top.get(), |p| p.energy_impact, |p| p.pid),
                    "apps_by_energy": ranked(&apps, top.get(), |p| p.energy_impact, |p| p.app_pid)
                },
                "skipped": skipped
            }),
        })
    })
}

fn usages(
    source: &Source,
    processes: &BTreeMap<u32, ps::SnapshotProcess>,
    euid: u32,
    baseline: bool,
) -> BTreeMap<u32, Usage> {
    processes
        .values()
        .filter(|p| policy::may_read_footprint(euid, p.uid))
        .map(|p| {
            let name = if baseline {
                format!("now-rusage-before-{}", p.pid)
            } else {
                format!("now-rusage-{}", p.pid)
            };
            let reading = source.read(&name, || direct::rusage::read_pid(p.pid));
            (
                p.pid,
                Usage {
                    reading,
                    at: Instant::now(),
                },
            )
        })
        .collect()
}

fn usage_counters(usage: Option<&Usage>) -> Option<policy::disk::Counters> {
    match &usage?.reading {
        Reading::Available { data } => Some(policy::disk::Counters {
            read_bytes: Some(data.read_bytes),
            written_bytes: Some(data.written_bytes),
        }),
        Reading::Unavailable { .. } => None,
    }
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
