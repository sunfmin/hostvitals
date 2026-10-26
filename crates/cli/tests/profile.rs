use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hostvitals-profile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();
        let standin = PathBuf::from(env!("CARGO_BIN_EXE_hostvitals"))
            .parent()
            .unwrap()
            .join("examples/probe-cli");
        std::os::unix::fs::symlink(standin, dir.join("bin/sample")).unwrap();
        let lab = Self(dir);
        lab.write(json!({"stdout": include_str!("../../policy/tests/fixtures/sample-busy.txt")}));
        lab
    }
    fn write(&self, value: Value) {
        fs::write(self.0.join("sample.json"), value.to_string()).unwrap();
    }
    fn run(&self, args: &[&str]) -> (std::process::Output, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(args)
            .env("PATH", self.0.join("bin"))
            .env("TMPDIR", &self.0)
            .env("HOSTVITALS_PROBE_FIXTURES", &self.0)
            .output()
            .unwrap();
        let answer = serde_json::from_slice(&out.stdout).unwrap();
        (out, answer)
    }
    fn calls(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.join("sample.json")).unwrap()).unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn profile_answers_rankings_and_retains_exact_private_distinct_reports() {
    let lab = Lab::new();
    let (out, answer) = lab.run(&["profile", "123", "--seconds", "2", "--top", "1"]);
    assert!(out.status.success(), "{answer}");
    assert_eq!(answer["status"], "available");
    assert_eq!(answer["pid"], 123);
    assert_eq!(answer["seconds"], 2);
    assert_eq!(answer["top"], 1);
    assert_eq!(answer["total_samples"], 200);
    assert_eq!(answer["hot_paths"].as_array().unwrap().len(), 1);
    assert_eq!(answer["hot_paths"][0]["sample_count"], 100);
    assert_eq!(answer["hot_paths"][0]["share"], 0.5);
    assert_eq!(answer["top_frames"][0]["sample_count"], 180);
    assert_eq!(answer["mostly_waiting"], false);
    let path = Path::new(answer["report_path"].as_str().unwrap());
    assert!(path.starts_with(lab.0.join("hostvitals")));
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        include_str!("../../policy/tests/fixtures/sample-busy.txt")
    );
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let calls = lab.calls();
    assert_eq!(
        calls["calls"][0],
        json!(["123", "2", "1", "-mayDie", "-file", path.to_str().unwrap()])
    );
    let (_, next) = lab.run(&["profile", "123"]);
    assert_eq!(next["seconds"], 5);
    assert_eq!(next["top"], 10);
    assert_ne!(next["report_path"], answer["report_path"]);
    assert!(path.exists());
}

#[test]
fn waiting_and_ended_processes_summarize_retained_reports_instead_of_progress() {
    let lab = Lab::new();
    let waiting = include_str!("../../policy/tests/fixtures/sample-waiting.txt");
    lab.write(json!({"stdout":"Sampling process 123...\n", "report":waiting}));
    let (out, answer) = lab.run(&["profile", "123"]);
    assert!(out.status.success(), "{answer}");
    assert_eq!(answer["status"], "available");
    assert_eq!(answer["total_samples"], 100);
    assert_eq!(answer["waiting_samples"], 90);
    assert_eq!(answer["mostly_waiting"], true);
    assert_eq!(answer["observation"], "mostly waiting");
    assert_eq!(answer["hot_paths"][0]["sample_count"], 10);
    assert_eq!(answer["top_frames"][0]["sample_count"], 90);
    assert_eq!(answer["partial"], false);
    assert_eq!(
        fs::read_to_string(answer["report_path"].as_str().unwrap()).unwrap(),
        waiting
    );

    let ended = include_str!("../../policy/tests/fixtures/sample-ended.txt");
    for report in [ended, ended.split("Total number in stack").next().unwrap()] {
        lab.write(json!({"report":report, "stdout":"Process 123 exited while sampling", "exit_code":1, "stderr":"Process exited while sampling"}));
        let (out, answer) = lab.run(&["profile", "123"]);
        assert!(out.status.success(), "{answer}");
        assert_eq!(answer["status"], "available");
        assert_eq!(answer["partial"], true);
        assert_eq!(answer["total_samples"], 26);
        assert_eq!(answer["waiting_samples"], 26);
        assert_eq!(answer["mostly_waiting"], true);
        assert_eq!(answer["hot_paths"], json!([]));
        assert_eq!(answer["top_frames"][0]["sample_count"], 26);
        assert!(
            answer["diagnostic"]
                .as_str()
                .unwrap()
                .contains("Process exited")
        );
        assert_eq!(
            fs::read_to_string(answer["report_path"].as_str().unwrap()).unwrap(),
            report
        );
    }
    lab.write(json!({"report":ended.split("Total number in stack").next().unwrap()}));
    let (_, answer) = lab.run(&["profile", "123"]);
    assert_eq!(answer["status"], "available");
    assert_eq!(answer["partial"], true);
    assert_eq!(answer["total_samples"], 26);
    assert!(answer["diagnostic"].is_null());
}

#[test]
fn denied_missing_and_malformed_reads_are_skipped_with_retry_lines_and_no_fake_counts() {
    let lab = Lab::new();
    lab.write(json!({"exit_code":255,"stderr":"sample[41785]: sample cannot examine process 123 because you do not have appropriate privileges to examine it; try running with `sudo`."}));
    let (out, answer) = lab.run(&["profile", "123", "--seconds", "2", "--top", "3"]);
    assert!(out.status.success(), "{answer}");
    assert_eq!(answer["status"], "skipped");
    assert_eq!(
        answer["fix"],
        "sudo hostvitals profile 123 --seconds 2 --top 3"
    );
    assert!(answer["reason"].as_str().unwrap().contains("privileges"));
    assert!(Path::new(answer["report_path"].as_str().unwrap()).exists());
    for key in [
        "total_samples",
        "waiting_samples",
        "mostly_waiting",
        "hot_paths",
        "top_frames",
    ] {
        assert!(answer.get(key).is_none(), "{key}: {answer}");
    }
    lab.write(json!({"report":"unrecognized report", "stdout":include_str!("../../policy/tests/fixtures/sample-busy.txt")}));
    let (out, answer) = lab.run(&["profile", "123"]);
    assert!(out.status.success());
    assert_eq!(answer["status"], "skipped");
    assert!(answer["fix"].as_str().unwrap().starts_with("brew upgrade"));
    assert!(answer.get("total_samples").is_none());
    assert_eq!(
        fs::read_to_string(answer["report_path"].as_str().unwrap()).unwrap(),
        "unrecognized report"
    );

    for report in ["", "Call graph:\n\nBinary Images:\n"] {
        lab.write(json!({"report":report}));
        let (out, answer) = lab.run(&["profile", "123"]);
        assert!(out.status.success());
        assert_eq!(answer["status"], "skipped");
        assert!(
            answer["reason"]
                .as_str()
                .unwrap()
                .contains("no call stacks collected")
        );
        assert_eq!(answer["fix"], "hostvitals profile 123 --seconds 5 --top 10");
        for key in [
            "total_samples",
            "waiting_samples",
            "mostly_waiting",
            "hot_paths",
            "top_frames",
        ] {
            assert!(answer.get(key).is_none(), "{key}: {answer}");
        }
        let path = answer["report_path"].as_str().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), report);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    fs::remove_file(lab.0.join("bin/sample")).unwrap();
    let (out, answer) = lab.run(&["profile", "123"]);
    assert!(out.status.success());
    assert_eq!(answer["status"], "skipped");
    assert!(answer["report_path"].is_null());
    assert!(answer["fix"].as_str().unwrap().starts_with("PATH=/usr/bin"));
    assert!(answer.get("hot_paths").is_none());
}

#[test]
fn unsafe_report_directories_are_never_reused_or_modified() {
    let lab = Lab::new();
    let root = lab.0.join("hostvitals");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    let (out, answer) = lab.run(&["profile", "123"]);
    assert!(out.status.success());
    assert_eq!(answer["status"], "skipped");
    assert!(answer["reason"].as_str().unwrap().contains("0700"));
    assert!(answer["fix"].as_str().unwrap().contains("TMPDIR="));
    assert!(answer["report_path"].is_null());
    assert!(lab.calls()["calls"].is_null());
    assert_eq!(
        fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);

    fs::remove_dir(&root).unwrap();
    let destination = lab.0.join("unrelated");
    fs::create_dir(&destination).unwrap();
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o700)).unwrap();
    std::os::unix::fs::symlink(&destination, &root).unwrap();
    let (_, answer) = lab.run(&["profile", "123"]);
    assert_eq!(answer["status"], "skipped");
    assert!(answer["report_path"].is_null());
    assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    assert!(fs::symlink_metadata(root).unwrap().file_type().is_symlink());
}

#[test]
fn profile_help_documents_every_key_and_retention_and_invalid_arguments_refuse() {
    for args in [["profile", "--help"], ["help", "profile"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success());
        let help = String::from_utf8(out.stdout).unwrap();
        for key in [
            "pid",
            "seconds",
            "top",
            "status",
            "report_path",
            "total_samples",
            "waiting_samples",
            "mostly_waiting",
            "observation",
            "partial",
            "diagnostic",
            "hot_paths",
            "frames",
            "sample_count",
            "share",
            "top_frames",
            "frame",
            "reason",
            "fix",
            "$TMPDIR/hostvitals/",
            "0600",
            "0700",
            "remove",
            "purged",
        ] {
            assert!(help.contains(key), "{key}: {help}");
        }
    }
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .arg("--help")
        .output()
        .unwrap();
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(!help.contains("Every command only reads; none changes anything"));
    assert!(help.contains("report"));
    for args in [
        vec!["profile"],
        vec!["profile", "0"],
        vec!["profile", "2147483648"],
        vec!["profile", "123", "--seconds", "0"],
        vec!["profile", "123", "--top", "0"],
        vec!["profile", "123", "--seconds", "1.5"],
        vec!["profile", "123", "--top", "-1"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success());
        let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(answer.as_object().unwrap().len(), 1);
        assert!(
            answer["error"]
                .as_str()
                .unwrap()
                .contains("hostvitals --help")
        );
    }
}

#[test]
fn oversized_reports_remain_complete_on_disk_and_are_never_summarized() {
    let lab = Lab::new();
    let mut report = include_str!("../../policy/tests/fixtures/sample-busy.txt").to_owned();
    report.push_str(&"x".repeat(hostvitals::tools::MEASUREMENT_LIMIT_MIB * 1024 * 1024));
    lab.write(json!({"report":report}));
    let (out, answer) = lab.run(&["profile", "123"]);
    assert!(out.status.success());
    assert_eq!(answer["status"], "skipped");
    assert!(answer["reason"].as_str().unwrap().contains("exceeded"));
    assert!(answer.get("total_samples").is_none());
    assert_eq!(
        fs::read_to_string(answer["report_path"].as_str().unwrap()).unwrap(),
        report
    );
}

#[test]
fn live_ended_process_retains_the_same_report_that_was_summarized() {
    let lab = Lab::new();
    let target = lab.0.join("bin/profile-target");
    fs::rename(lab.0.join("bin/sample"), &target).unwrap();
    // Delay observer startup beyond the old child's entire lifetime. The target
    // stays asleep until sample announces collection, then is released below.
    let sample = lab.0.join("bin/sample");
    // A pseudo-terminal makes sample's progress line visible before exit;
    // script flushes it immediately rather than buffering it for 30 seconds.
    fs::write(&sample, "#!/bin/sh\n/bin/sleep 0.6\nexec /usr/bin/script -qFe \"$HOSTVITALS_LIVE_SAMPLE_PROGRESS\" /usr/bin/sample \"$@\"\n").unwrap();
    fs::set_permissions(&sample, fs::Permissions::from_mode(0o700)).unwrap();
    let mut child = ReapedChild(Command::new(target).arg(&lab.0).spawn().unwrap());
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while !lab.0.join("target-ready").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "target exited before readiness"
        );
        assert!(
            Instant::now() < ready_deadline,
            "target did not become ready"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let progress = lab.0.join("progress.txt");
    let answer_path = lab.0.join("answer.json");
    let mut observer = ReapedChild(
        Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(["profile", &child.0.id().to_string(), "--seconds", "3"])
            .env("TMPDIR", &lab.0)
            .env(
                "PATH",
                format!(
                    "{}:/usr/bin:/bin:/usr/sbin:/sbin",
                    lab.0.join("bin").display()
                ),
            )
            .env("HOSTVITALS_LIVE_SAMPLE_PROGRESS", &progress)
            .env_remove("HOSTVITALS_PROBE_FIXTURES")
            .stdout(fs::File::create(&answer_path).unwrap())
            .spawn()
            .unwrap(),
    );
    let collection_deadline = Instant::now() + Duration::from_secs(10);
    while !fs::read_to_string(&progress)
        .unwrap_or_default()
        .contains("Sampling process")
    {
        assert!(
            observer.0.try_wait().unwrap().is_none(),
            "observer exited before collection: {}",
            fs::read_to_string(&answer_path).unwrap()
        );
        assert!(
            Instant::now() < collection_deadline,
            "sample did not announce collection: {}",
            fs::read_to_string(&progress).unwrap_or_default()
        );
        thread::sleep(Duration::from_millis(10));
    }
    let collecting = Instant::now();
    thread::sleep(Duration::from_millis(250));
    assert!(child.0.try_wait().unwrap().is_none());
    fs::write(lab.0.join("target-release"), "").unwrap();
    let completion_deadline = Instant::now() + Duration::from_secs(10);
    while child.0.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < completion_deadline,
            "profile did not finish after target release"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(child.0.wait().unwrap().success());
    let ended_after = collecting.elapsed();
    assert!(
        ended_after < Duration::from_secs(3),
        "target must end before the requested Window completes"
    );
    let observation_deadline = Instant::now() + Duration::from_secs(40);
    while observer.0.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < observation_deadline,
            "observer did not finish"
        );
        thread::sleep(Duration::from_millis(10));
    }
    let answer: Value = serde_json::from_slice(&fs::read(answer_path).unwrap()).unwrap();
    let path = answer["report_path"].as_str().unwrap();
    let text = fs::read_to_string(path).unwrap();
    eprintln!("retained live report before cleanup:\n{text}");
    assert!(observer.0.wait().unwrap().success(), "{answer}\n{text}");
    assert_eq!(answer["status"], "available", "{answer}\n{text}");
    assert!(answer["total_samples"].as_u64().unwrap() > 0);
    assert!(answer["waiting_samples"].as_u64().unwrap() > 0);
    assert_eq!(answer["mostly_waiting"], true);
    let summary = hostvitals_policy::profile::summarize(&text, 10).unwrap();
    assert_eq!(answer["total_samples"], summary.total_samples);
    assert_eq!(answer["waiting_samples"], summary.waiting_samples);
    assert_eq!(
        answer["top_frames"],
        json!(
            summary
                .top_frames
                .iter()
                .map(|frame| json!({"frame":frame.frame,"sample_count":frame.sample_count}))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        answer["hot_paths"],
        json!(summary.hot_paths.iter().map(|path| json!({"frames":path.frames,"sample_count":path.sample_count,"share":path.share})).collect::<Vec<_>>())
    );
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(Path::new(path).parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    eprintln!(
        "live profile: {} samples, {} waiting; target ended {} ms after collection announcement",
        summary.total_samples,
        summary.waiting_samples,
        ended_after.as_millis()
    );
}

struct ReapedChild(Child);

impl Drop for ReapedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
