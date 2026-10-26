//! Complete single-Process observation, with optional measurements explicitly skipped.
use crate::{
    direct::{self, Reading, Source},
    tools::{self, ps},
};
use hostvitals_policy as policy;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn geteuid() -> u32;
}
pub const DEFAULT_SECONDS: u64 = 3;

pub fn answer(source: &Source, pid: u32, seconds: u64) -> Result<Value, String> {
    let retry = format!("hostvitals process {pid} --seconds {seconds}");
    let root_retry = format!("sudo {retry}");
    let before = ps::snapshot().map_err(|e| format!("process {pid}: {e}; run {retry}"))?;
    let old = before
        .get(&pid)
        .ok_or_else(|| format!("process {pid} does not exist; run hostvitals now"))?;
    let start = Instant::now();
    let euid = match source.read("process-owner", || Reading::Available {
        data: unsafe { geteuid() },
    }) {
        Reading::Available { data } => data,
        Reading::Unavailable { .. } => {
            return Err(format!(
                "observer identity unavailable for process {pid}; run hostvitals doctor"
            ));
        }
    };
    let accessible = policy::may_read_footprint(euid, old.uid);
    let initial_files = if euid == 0 {
        tools::lsof::read(pid)
            .map(|inventory| inventory.files)
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let usage_before =
        accessible.then(|| source.read("process-rusage-0", || direct::rusage::read_pid(pid)));
    let disk_start = Instant::now();
    // Scoped work cannot survive an error or an exited/reused PID.
    let (usage_after, disk_window, after, window, activity) = std::thread::scope(|scope| {
        let activity = (euid == 0)
            .then(|| scope.spawn(|| tools::fs_usage::read(pid, seconds, &initial_files)));
        std::thread::sleep(Duration::from_secs(seconds));
        let usage_after =
            accessible.then(|| source.read("process-rusage-1", || direct::rusage::read_pid(pid)));
        let disk_window = disk_start.elapsed().as_secs_f64();
        let after = ps::snapshot();
        let window = start.elapsed().as_secs_f64();
        let activity = activity.map(|handle| {
            handle
                .join()
                .unwrap_or_else(|_| Err("fs_usage reader failed".into()))
        });
        (usage_after, disk_window, after, window, activity)
    });
    let after = after.map_err(|e| format!("process {pid}: {e}; run {retry}"))?;
    let p = after
        .get(&pid)
        .ok_or_else(|| format!("process {pid} exited during the window; run hostvitals now"))?;
    if old.started_at != p.started_at || old.uid != p.uid {
        return Err(format!(
            "process {pid} was replaced during the window; run {retry}"
        ));
    }
    let mut skipped = Vec::new();
    let cpu_percent = policy::counter_delta(old.cpu_hundredths, p.cpu_hundredths, true)
        .map(|n| policy::cpu_percent(n, window));
    if cpu_percent.is_none() {
        skip(
            &mut skipped,
            "cpu_percent",
            "cumulative CPU counter regressed",
            &retry,
        );
    }
    let mut answer = json!({
        "source":if source.0.is_some(){"fixtures"}else{"host"},
        "pid":pid,"parent_pid":p.parent_pid,"uid":p.uid,"user":p.user,"name":p.name,"started_at":p.started_at,
        "cpu_seconds":p.cpu_hundredths as f64/100.0,"cpu_percent":cpu_percent,"window_seconds":window,"resident_bytes":p.resident_bytes,
        "app":null,"command_line":null,"cwd":null,"footprint_bytes":null,"peak_footprint_bytes":null,
        "disk":{"read_bytes":null,"written_bytes":null,"window_read_bytes":null,"window_written_bytes":null,"window_seconds":null},
        "energy_nj":null,"threads":null,"open_files":null,"sockets":null,"file_activity":null
    });
    if accessible {
        let usage_before = value(
            usage_before.unwrap(),
            "disk.baseline",
            &root_retry,
            &mut skipped,
        );
        let usage_after = value(usage_after.unwrap(), "rusage", &root_retry, &mut skipped);
        if let Some(usage) = usage_after {
            answer["footprint_bytes"] = json!(usage.footprint_bytes);
            answer["peak_footprint_bytes"] = json!(usage.peak_footprint_bytes);
            answer["energy_nj"] = json!(usage.energy_nj);
            let read_delta = usage_before
                .as_ref()
                .and_then(|old| policy::counter_delta(old.read_bytes, usage.read_bytes, true));
            let write_delta = usage_before.as_ref().and_then(|old| {
                policy::counter_delta(old.written_bytes, usage.written_bytes, true)
            });
            if usage_before.is_some() && (read_delta.is_none() || write_delta.is_none()) {
                skip(
                    &mut skipped,
                    "disk.window_bytes",
                    "disk counter regressed",
                    &retry,
                );
            }
            answer["disk"] = json!({"read_bytes":usage.read_bytes,"written_bytes":usage.written_bytes,"window_read_bytes":read_delta,"window_written_bytes":write_delta,"window_seconds":disk_window});
        }
        if let Some(args) = value(
            direct::arguments::read(source, pid),
            "command_line",
            &root_retry,
            &mut skipped,
        ) {
            answer["command_line"] = json!(args);
        }
        if let Some(task) = value(
            source.read("process-task", || direct::libproc::read_pid(pid)),
            "threads",
            &root_retry,
            &mut skipped,
        ) {
            if task.pid == pid {
                answer["threads"] = json!(task.threads);
            } else {
                skip(&mut skipped, "threads", "invalid task identity", &retry);
            }
        }
        if let Some(app) = value(
            source.read("process-app", || direct::responsibility::read_pid(pid)),
            "app",
            "brew upgrade sunfmin/tap/hostvitals && hostvitals doctor",
            &mut skipped,
        ) {
            if app.pid == pid && app.app_pid > 0 {
                let identity = after.get(&app.app_pid);
                answer["app"] = json!({"app_pid":app.app_pid,"name":identity.map(|p| &p.name),"started_at":identity.map(|p| &p.started_at)});
            } else {
                skip(
                    &mut skipped,
                    "app",
                    "invalid responsibility identity",
                    &retry,
                );
            }
        }
        match tools::lsof::read(pid) {
            Ok(inventory) => {
                answer["cwd"] = json!(inventory.cwd);
                if answer["cwd"].is_null() {
                    skip(
                        &mut skipped,
                        "cwd",
                        "lsof did not report a working directory",
                        &root_retry,
                    );
                }
                answer["open_files"] = json!(
                    inventory
                        .files
                        .iter()
                        .map(|f| json!({"fd":f.fd,"kind":f.kind,"path":f.path}))
                        .collect::<Vec<_>>()
                );
                let names = tools::names::resolve(
                    inventory.sockets.iter().filter_map(|s| s.remote_ip.clone()),
                );
                answer["sockets"] = json!(inventory.sockets.iter().map(|s| json!({"fd":s.fd,"kind":s.kind,"protocol":s.protocol,"local":s.local,"remote_ip":s.remote_ip,"remote_port":s.remote_port,"remote_name":s.remote_ip.as_ref().and_then(|ip| names.get(ip)),"state":s.state})).collect::<Vec<_>>());
            }
            Err(_) => skip(
                &mut skipped,
                "cwd,open_files,sockets",
                "lsof denied, missing, oversized or unrecognized; retry after updating hostvitals if root is also denied",
                &format!("PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH {root_retry}"),
            ),
        }
    } else {
        for measurement in [
            "app",
            "command_line",
            "cwd",
            "footprint_bytes",
            "peak_footprint_bytes",
            "disk",
            "energy_nj",
            "threads",
            "open_files",
            "sockets",
        ] {
            skip(
                &mut skipped,
                measurement,
                "another user's process requires root for this measurement",
                &root_retry,
            );
        }
    }
    match activity {
        Some(Ok(events)) => {
            if events.iter().any(|e| e.path.is_none()) {
                skip(
                    &mut skipped,
                    "file_activity.path",
                    "fs_usage omitted a path and its descriptor could not be resolved reliably",
                    &root_retry,
                );
            }
            answer["file_activity"] = json!(
                events
                    .iter()
                    .map(|e| json!({"operation":e.operation,"path":e.path,"bytes":e.bytes}))
                    .collect::<Vec<_>>()
            )
        }
        Some(Err(_)) => skip(
            &mut skipped,
            "file_activity",
            "fs_usage denied, missing, exceeded bounds or reported an unsupported/incomplete event stream",
            &format!("PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH {root_retry}"),
        ),
        None => skip(
            &mut skipped,
            "file_activity",
            "fs_usage requires root",
            &root_retry,
        ),
    }
    answer["skipped"] = json!(skipped);
    Ok(answer)
}
fn value<T>(
    reading: Reading<T>,
    measurement: &str,
    fix: &str,
    skipped: &mut Vec<Value>,
) -> Option<T> {
    match reading {
        Reading::Available { data } => Some(data),
        Reading::Unavailable { reason } => {
            skip(skipped, measurement, &reason, fix);
            None
        }
    }
}
fn skip(skipped: &mut Vec<Value>, measurement: &str, reason: &str, fix: &str) {
    skipped.push(json!({"measurement":measurement,"reason":reason,"fix":fix}));
}
