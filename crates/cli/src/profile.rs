//! Presents a Profile of the same sample report retained for the caller.
use crate::tools::sample;
use hostvitals_policy::profile;
use serde_json::{Value, json};

pub(crate) const DEFAULT_SECONDS: u64 = 5;
pub(crate) const DEFAULT_TOP: u32 = 10;

pub(crate) fn answer(pid: u32, seconds: u64, top: u32) -> Value {
    let command = format!("hostvitals profile {pid} --seconds {seconds} --top {top}");
    let mut answer = json!({"pid":pid,"seconds":seconds,"top":top,"report_path":null});
    let report = match sample::read(pid, seconds) {
        Ok(report) => report,
        Err(reason) => return skipped(answer, reason, &command),
    };
    answer["report_path"] = json!(report.path);
    let text = match report.text {
        Ok(text) => text,
        Err(reason) => return skipped(answer, reason, &command),
    };
    match profile::summarize(&text, top as usize) {
        Ok(summary) => {
            let hot_paths: Vec<_> = summary.hot_paths.into_iter().map(|path| {
                json!({"frames":path.frames,"sample_count":path.sample_count,"share":path.share})
            }).collect();
            let top_frames: Vec<_> = summary
                .top_frames
                .into_iter()
                .map(|frame| json!({"frame":frame.frame,"sample_count":frame.sample_count}))
                .collect();
            answer["status"] = json!("available");
            answer["total_samples"] = json!(summary.total_samples);
            answer["waiting_samples"] = json!(summary.waiting_samples);
            answer["mostly_waiting"] = json!(summary.mostly_waiting);
            answer["observation"] = json!(if summary.mostly_waiting {
                "mostly waiting"
            } else {
                "sampled call stacks"
            });
            answer["partial"] = json!(summary.partial || report.failure.is_some());
            answer["diagnostic"] = json!(report.failure);
            answer["hot_paths"] = json!(hot_paths);
            answer["top_frames"] = json!(top_frames);
            answer
        }
        Err(reason) => skipped(
            answer,
            report
                .failure
                .unwrap_or_else(|| format!("unrecognized sample report: {reason}")),
            &command,
        ),
    }
}

fn skipped(mut answer: Value, reason: String, command: &str) -> Value {
    let lower = reason.to_ascii_lowercase();
    let fix = if reason.contains("not found on PATH") {
        format!("PATH=/usr/bin:/bin:/usr/sbin:/sbin:$PATH {command}")
    } else if reason.starts_with("cannot create private sample report:") {
        format!("TMPDIR=\"$(mktemp -d)\" {command}")
    } else if [
        "permission",
        "not permitted",
        "task_for_pid",
        "cannot examine",
        "not allowed",
        "unable to get",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        format!("sudo {command}")
    } else if reason.starts_with("unrecognized sample report:") {
        format!("brew upgrade sunfmin/tap/hostvitals && {command}")
    } else {
        command.to_owned()
    };
    answer["status"] = json!("skipped");
    answer["reason"] = json!(reason);
    answer["fix"] = json!(fix);
    answer
}
