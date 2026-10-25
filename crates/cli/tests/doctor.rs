use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hostvitals-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();
        let lab = Self(dir);
        lab.write(
            "host",
            json!({"macos_version":"27.0.1","arch":"arm64","euid":501}),
        );
        lab.write("rusage", json!({"status":"available","data":{"resident_bytes":4096,"footprint_bytes":2048,"peak_footprint_bytes":8192,"read_bytes":0,"written_bytes":0,"energy_nj":0}}));
        lab.write(
            "libproc",
            json!({"status":"available","data":{"pid":123,"resident_bytes":4096,"threads":2}}),
        );
        lab.write(
            "ioreport",
            json!({"status":"available","data":{"energy_channels":3,"sampled_channels":3}}),
        );
        lab.write(
            "responsibility",
            json!({"status":"available","data":{"pid":123,"app_pid":100}}),
        );
        lab
    }
    fn write(&self, name: &str, value: Value) {
        fs::write(self.0.join(format!("{name}.json")), value.to_string()).unwrap();
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(args)
            .env("HOSTVITALS_PROBE_FIXTURES", &self.0)
            .env("PATH", self.0.join("bin"))
            .output()
            .unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn stalled_and_verbose_cli_failures_are_bounded_and_do_not_hide_other_checks() {
    let lab = Lab::new();
    lab.install_clis();
    lab.write(
        "ps",
        json!({"exit_code":0,"stderr":"","calls":[],"delay_ms":12000}),
    );
    lab.write(
        "top",
        json!({"exit_code":77,"stderr":"x".repeat(1000000),"calls":[]}),
    );
    let start = std::time::Instant::now();
    let out = lab.run(&["doctor"]);
    assert!(out.status.success());
    assert!(start.elapsed() < std::time::Duration::from_secs(15));
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    let checks = answer["checks"].as_array().unwrap();
    let stalled = checks.iter().find(|c| c["name"] == "ps").unwrap();
    assert_eq!(stalled["status"], "unavailable");
    assert!(stalled["reason"].as_str().unwrap().contains("exceeded"));
    let verbose = checks.iter().find(|c| c["name"] == "top").unwrap();
    assert_eq!(verbose["status"], "unavailable");
    assert!(verbose["reason"].as_str().unwrap().len() < 4300);
    assert_eq!(
        checks
            .iter()
            .find(|c| c["name"] == "responsibility")
            .unwrap()["status"],
        "available"
    );
}

#[test]
fn doctor_reports_every_missing_cli() {
    let lab = Lab::new();
    let out = lab.run(&["doctor"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(answer["host"]["macos_version"], "27.0.1");
    assert_eq!(answer["host"]["arch"], "arm64");
    assert_eq!(answer["host"]["is_root"], false);
    let checks = answer["checks"].as_array().unwrap();
    for name in [
        "ps",
        "top",
        "nettop",
        "iostat",
        "sample",
        "vmmap",
        "footprint",
        "pmset",
        "powermetrics",
        "fs_usage",
    ] {
        let check = checks.iter().find(|c| c["name"] == name).unwrap();
        assert_eq!(check["status"], "unavailable", "{name}");
        assert!(check["fix"].as_str().unwrap().contains("PATH"), "{check}");
    }
    assert_eq!(checks.len(), 14);
}

impl Lab {
    fn install_clis(&self) {
        let standin = PathBuf::from(env!("CARGO_BIN_EXE_hostvitals"))
            .parent()
            .unwrap()
            .join("examples/probe-cli");
        assert!(
            standin.exists(),
            "build the example first: cargo build --example probe-cli"
        );
        for tool in hostvitals::tools::Tool::all() {
            std::os::unix::fs::symlink(&standin, self.0.join("bin").join(tool.name())).unwrap();
            self.write(
                tool.name(),
                serde_json::to_value(hostvitals::tools::StandIn::default()).unwrap(),
            );
        }
    }
    fn calls(&self, name: &str) -> Vec<Vec<String>> {
        let state: hostvitals::tools::StandIn =
            serde_json::from_slice(&fs::read(self.0.join(format!("{name}.json"))).unwrap())
                .unwrap();
        state.calls
    }
}

#[test]
fn unprivileged_doctor_reads_available_clis_but_never_launches_root_tools() {
    let lab = Lab::new();
    lab.install_clis();
    let out = lab.run(&["doctor"]);
    assert!(out.status.success());
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    for tool in hostvitals::tools::Tool::all() {
        let check = answer["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == tool.name())
            .unwrap();
        if tool.needs_root() {
            assert_eq!(check["status"], "skipped", "{check}");
            assert_eq!(check["fix"], "sudo hostvitals doctor");
            assert!(lab.calls(tool.name()).is_empty());
        } else {
            assert_eq!(check["status"], "available", "{check}");
            assert_eq!(lab.calls(tool.name()).len(), 1, "{}", tool.name());
            assert_ne!(lab.calls(tool.name())[0], ["-h"]);
        }
    }
}

#[test]
#[cfg(target_os = "macos")]
fn real_host_smoke_has_bounded_native_observations() {
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .arg("doctor")
        .env_remove("HOSTVITALS_PROBE_FIXTURES")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(answer["source"], "host");
    assert!(!answer["host"]["macos_version"].as_str().unwrap().is_empty());
    assert!(["arm64", "x86_64"].contains(&answer["host"]["arch"].as_str().unwrap()));
    assert!(answer["host"]["is_root"].is_boolean());
    for name in ["rusage", "libproc", "ioreport", "responsibility"] {
        let check = answer["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        // Private interfaces and rusage v6 vary by OS and hardware. Their absence
        // is a supported doctor answer, not a failure of the live smoke test.
        if check["status"] == "unavailable" {
            assert!(!check["reason"].as_str().unwrap().is_empty());
            assert!(!check["fix"].as_str().unwrap().is_empty());
            assert!(check.get("data").is_none());
            continue;
        }
        assert_eq!(check["status"], "available", "{check}");
        let data = &check["data"];
        match name {
            "rusage" => {
                assert!(data["resident_bytes"].as_u64().unwrap() > 0);
                assert!(
                    data["peak_footprint_bytes"].as_u64().unwrap()
                        >= data["footprint_bytes"].as_u64().unwrap()
                );
                for field in ["read_bytes", "written_bytes", "energy_nj"] {
                    assert!(data[field].is_u64());
                }
            }
            "libproc" => {
                assert!(data["pid"].as_u64().unwrap() > 0);
                assert!(data["threads"].as_u64().unwrap() > 0);
            }
            "ioreport" => {
                assert!(data["energy_channels"].as_u64().unwrap() > 0);
                assert!(data["sampled_channels"].as_u64().unwrap() > 0);
            }
            "responsibility" => {
                assert!(data["pid"].as_u64().unwrap() > 0);
                assert!(data["app_pid"].as_u64().unwrap() > 0);
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn root_doctor_collects_all_failures_and_all_fixture_readings() {
    let lab = Lab::new();
    lab.install_clis();
    lab.write(
        "host",
        json!({"macos_version":"27.0.1","arch":"x86_64","euid":0}),
    );
    for name in ["ps", "powermetrics"] {
        lab.write(
            name,
            json!({"exit_code":77,"stderr":"permission denied by fixture","calls":[]}),
        );
    }
    lab.write(
        "ioreport",
        json!({"status":"unavailable","reason":"private symbol removed"}),
    );
    let out = lab.run(&["doctor"]);
    assert!(out.status.success());
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(answer["source"], "fixtures");
    assert_eq!(answer["host"]["is_root"], true);
    let checks = answer["checks"].as_array().unwrap();
    for name in ["ps", "powermetrics", "ioreport"] {
        let check = checks.iter().find(|c| c["name"] == name).unwrap();
        assert_eq!(check["status"], "unavailable", "{check}");
        assert!(!check["reason"].as_str().unwrap().is_empty());
        assert!(check["fix"].as_str().unwrap().contains("hostvitals doctor"));
    }
    assert_eq!(
        checks.iter().find(|c| c["name"] == "rusage").unwrap()["data"]["resident_bytes"],
        4096
    );
    assert_eq!(
        checks.iter().find(|c| c["name"] == "libproc").unwrap()["data"]["threads"],
        2
    );
    assert_eq!(
        checks
            .iter()
            .find(|c| c["name"] == "responsibility")
            .unwrap()["data"]["app_pid"],
        100
    );
    for tool in hostvitals::tools::Tool::all() {
        assert_eq!(lab.calls(tool.name()).len(), 1);
    }
}

#[test]
fn native_fixture_failures_are_independent_and_never_fall_back_to_the_host() {
    let lab = Lab::new();
    for name in ["rusage", "libproc", "ioreport", "responsibility"] {
        lab.write(
            name,
            json!({"status":"unavailable","reason":format!("{name} unavailable by fixture")}),
        );
    }
    let out = lab.run(&["doctor"]);
    assert!(out.status.success());
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    for name in ["rusage", "libproc", "ioreport", "responsibility"] {
        let check = answer["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        assert_eq!(check["status"], "unavailable");
        assert_eq!(check["reason"], format!("{name} unavailable by fixture"));
        assert!(check.get("data").is_none());
    }
    fs::remove_file(lab.0.join("rusage.json")).unwrap();
    fs::write(lab.0.join("libproc.json"), "bad json").unwrap();
    let out = lab.run(&["doctor"]);
    let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
    for name in ["rusage", "libproc"] {
        let check = answer["checks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap();
        assert_eq!(check["status"], "unavailable");
        assert!(check["reason"].as_str().unwrap().starts_with("fixture"));
    }
}
