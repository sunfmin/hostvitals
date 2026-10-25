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
            "hostvitals-memory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();
        let lab = Self(dir);
        let standin = PathBuf::from(env!("CARGO_BIN_EXE_hostvitals"))
            .parent()
            .unwrap()
            .join("examples/probe-cli");
        for name in ["vmmap", "footprint"] {
            std::os::unix::fs::symlink(&standin, lab.0.join("bin").join(name)).unwrap();
        }
        lab.write(
            "vmmap",
            json!({"stdout": include_str!("../../policy/tests/fixtures/vmmap-regions.txt")}),
        );
        lab.write(
            "footprint",
            json!({"stdout": include_str!("../../policy/tests/fixtures/footprint.txt")}),
        );
        lab
    }
    fn write(&self, tool: &str, state: Value) {
        fs::write(self.0.join(format!("{tool}.json")), state.to_string()).unwrap();
    }
    fn run(&self, args: &[&str]) -> (std::process::Output, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(args)
            .env("PATH", self.0.join("bin"))
            .env("HOSTVITALS_PROBE_FIXTURES", &self.0)
            .output()
            .unwrap();
        let answer = serde_json::from_slice(&out.stdout).unwrap();
        (out, answer)
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn memory_answers_one_json_object_with_all_sections_and_requested_top() {
    let lab = Lab::new();
    let (out, answer) = lab.run(&["memory", "29682", "--top", "2"]);
    assert!(out.status.success(), "{answer}");
    assert_eq!(answer["pid"], 29682);
    assert_eq!(answer["top"], 2);
    assert_eq!(answer["footprint"]["bytes"], 1966488);
    assert_eq!(answer["region_kinds"]["status"], "available");
    assert_eq!(answer["malloc_zones"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(
        answer["largest_regions"]["items"].as_array().unwrap().len(),
        2
    );
    let (_, default) = lab.run(&["memory", "29682"]);
    assert_eq!(default["top"], 15);
    assert_eq!(
        default["largest_regions"]["items"]
            .as_array()
            .unwrap()
            .len(),
        15
    );
}

#[test]
fn denied_vmmap_preserves_footprint_and_names_the_root_retry() {
    let lab = Lab::new();
    lab.write("vmmap", json!({"exit_code":1,"stderr":"vmmap cannot examine process 123 because you do not have appropriate privileges; try running with sudo"}));
    let (out, answer) = lab.run(&["memory", "123", "--top", "7"]);
    assert!(out.status.success(), "{answer}");
    assert_eq!(answer["footprint"]["bytes"], 1966488);
    for section in ["region_kinds", "malloc_zones", "largest_regions"] {
        assert_eq!(answer[section]["status"], "skipped");
        assert_eq!(answer[section]["fix"], "sudo hostvitals memory 123 --top 7");
        assert!(
            answer[section]["reason"]
                .as_str()
                .unwrap()
                .contains("appropriate privileges")
        );
        assert!(answer[section].get("items").is_none());
    }
    let state: Value =
        serde_json::from_slice(&fs::read(lab.0.join("vmmap.json")).unwrap()).unwrap();
    assert_eq!(
        state["calls"],
        json!([["-summary", "123"], ["-wide", "-noCoalesce", "123"]])
    );
}

#[test]
fn missing_and_malformed_sources_never_become_zero_measurements() {
    let lab = Lab::new();
    fs::remove_file(lab.0.join("bin/vmmap")).unwrap();
    lab.write("footprint", json!({"stdout":"unsupported format"}));
    let (out, answer) = lab.run(&["memory", "123"]);
    assert!(out.status.success());
    assert_eq!(answer["footprint"]["status"], "skipped");
    assert!(answer["footprint"].get("bytes").is_none());
    assert!(
        answer["footprint"]["fix"]
            .as_str()
            .unwrap()
            .contains("brew upgrade")
    );
    assert!(
        answer["region_kinds"]["fix"]
            .as_str()
            .unwrap()
            .contains("PATH=")
    );
    assert_eq!(answer["largest_regions"]["status"], "skipped");
}

#[test]
fn memory_help_covers_every_returned_key_and_invalid_requests_refuse() {
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .args(["memory", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for key in [
        "pid",
        "top",
        "footprint",
        "region_kinds",
        "malloc_zones",
        "largest_regions",
        "status",
        "reason",
        "fix",
        "items",
        "kind",
        "zone",
        "memory",
        "regions",
        "allocations",
        "allocated_bytes",
        "bytes",
        "virtual_bytes",
        "resident_bytes",
        "dirty_bytes",
        "swapped_bytes",
        "clean_bytes",
        "address",
        "detail",
        "dirty_swapped_bytes",
    ] {
        assert!(help.contains(key), "missing {key}: {help}");
    }
    assert!(!help.contains("{const:"));
    let lab = Lab::new();
    for args in [
        vec!["memory"],
        vec!["memory", "0"],
        vec!["memory", "-1"],
        vec!["memory", "abc"],
        vec!["memory", "1", "--top", "0"],
        vec!["memory", "1", "--top", "-1"],
    ] {
        let (out, answer) = lab.run(&args);
        assert!(!out.status.success(), "{args:?}");
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
fn footprint_failure_preserves_vmmap_and_oversized_reports_are_not_parsed() {
    let lab = Lab::new();
    lab.write(
        "footprint",
        json!({"exit_code":1,"stderr":"process has exited"}),
    );
    let (out, answer) = lab.run(&["memory", "123"]);
    assert!(out.status.success());
    assert_eq!(answer["footprint"]["status"], "skipped");
    assert_eq!(answer["region_kinds"]["status"], "available");
    assert_eq!(answer["largest_regions"]["status"], "available");
    lab.write(
        "footprint",
        json!({"stdout":format!("phys_footprint: 1 B\n{}", "x".repeat(33*1024*1024))}),
    );
    let (out, answer) = lab.run(&["memory", "123"]);
    assert!(out.status.success());
    assert_eq!(answer["footprint"]["status"], "skipped");
    assert!(
        answer["footprint"]["reason"]
            .as_str()
            .unwrap()
            .contains("exceeded")
    );
    assert!(answer["footprint"].get("bytes").is_none());
}
