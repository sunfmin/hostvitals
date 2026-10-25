use serde_json::Value;
use serde_json::json;
use std::process::Command;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Lab(PathBuf);
impl Lab {
    fn new(before: String, after: String) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hostvitals-now-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();
        let lab = Self(dir);
        let standin = PathBuf::from(env!("CARGO_BIN_EXE_hostvitals"))
            .parent()
            .unwrap()
            .join("examples/probe-cli");
        assert!(standin.exists(), "cargo build --example probe-cli first");
        std::os::unix::fs::symlink(&standin, lab.0.join("bin/ps")).unwrap();
        std::os::unix::fs::symlink(&standin, lab.0.join("bin/nettop")).unwrap();
        lab.write("ps", json!({"stdout_readings":[before,after]}));
        lab.write("nettop", json!({"stdout":",bytes_in,bytes_out,\n"}));
        for (i, ticks) in [[100, 200, 300, 0], [130, 210, 360, 0]]
            .into_iter()
            .enumerate()
        {
            lab.write(&format!("now-host-{i}"), json!({"status":"available","data":{"euid":501,"cpu_ticks":ticks,"logical_cores":8,"physical_cores":4,"load_average":[2.0,3.0,4.0],"memory":{"total_bytes":51539607552u64,"free_percent":12,"pressure":"warn","swap_used_bytes":8568459428u64,"swap_total_bytes":9663676416u64,"compressed_bytes":3221225472u64}}}));
        }
        lab
    }
    fn write(&self, name: &str, value: Value) {
        fs::write(self.0.join(format!("{name}.json")), value.to_string()).unwrap();
    }
    fn process(&self, pid: u32, app: u32) {
        self.write(
            &format!("now-responsibility-{pid}"),
            json!({"status":"available","data":{"pid":pid,"app_pid":app}}),
        );
        self.write(&format!("now-rusage-{pid}"),json!({"status":"available","data":{"resident_bytes":999,"footprint_bytes":1000,"peak_footprint_bytes":2000,"read_bytes":0,"written_bytes":0,"energy_nj":0}}));
    }
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(args)
            .env("HOSTVITALS_PROBE_FIXTURES", &self.0)
            .env("PATH", self.0.join("bin"))
            .output()
            .unwrap()
    }
    fn answer(&self) -> Value {
        let out = self.run(&["now", "--seconds", "1", "--top", "2"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(String::from_utf8_lossy(&out.stderr).contains("fixtures"));
        serde_json::from_slice(&out.stdout).unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn row(pid: u32, uid: u32, cpu: &str, rss: u64, name: &str) -> String {
    format!(
        "{pid} 1 {uid} {} {cpu} {rss} Mon Oct 12 17:34:33 2026 {name}\n",
        if uid == 0 { "root" } else { "owner" }
    )
}

#[test]
fn network_rates_include_root_and_rank_processes_and_complete_apps_without_dns() {
    let rows = row(10, 0, "0:00.00", 1, "/root daemon")
        + &row(20, 501, "0:00.00", 1, "/App")
        + &row(21, 501, "0:00.00", 1, "/helper")
        + &row(30, 501, "0:00.00", 1, "/idle");
    let lab = Lab::new(rows.clone(), rows);
    for (pid, app) in [(10, 10), (20, 20), (21, 20), (30, 30)] {
        lab.process(pid, app);
    }
    lab.write("nettop", json!({"stdout_readings":[
        ",bytes_in,bytes_out,\nroot daemon.10,9000000000,8000000000,\nApp.20,10000,20000,\nhelper,with.dots.21,30000,40000,\n",
        ",bytes_in,bytes_out,\nroot daemon.10,9000000600,8000000100,\nApp.20,10400,20500,\nhelper,with.dots.21,30300,40600,\n"
    ]}));
    let a = lab.answer();
    let seconds = a["host"]["network"]["window_seconds"].as_f64().unwrap();
    assert!(seconds >= 1.0);
    for (key, expected) in [
        ("bytes_in_per_second", 1300.0),
        ("bytes_out_per_second", 1200.0),
    ] {
        assert!((a["host"]["network"][key].as_f64().unwrap() * seconds - expected).abs() < 0.001);
    }
    let root = &a["processes"][0];
    assert_eq!(root["uid"], 0);
    assert!(
        (root["network"]["bytes_in_per_second"].as_f64().unwrap() * seconds - 600.0).abs() < 0.001
    );
    assert_eq!(a["processes"][3]["network"]["bytes_in_per_second"], 0.0);
    assert_eq!(a["rankings"]["processes_by_network_in"][0]["pid"], 10);
    assert_eq!(a["rankings"]["processes_by_network_out"][0]["pid"], 21);
    for key in [
        "processes_by_network_in",
        "processes_by_network_out",
        "apps_by_network_in",
        "apps_by_network_out",
    ] {
        assert_eq!(a["rankings"][key].as_array().unwrap().len(), 2);
    }
    let app = &a["rankings"]["apps_by_network_in"][0];
    assert_eq!(app["app_pid"], 20);
    assert_eq!(app["process_count"], 2);
    assert!(
        (app["network"]["bytes_in_per_second"].as_f64().unwrap() * seconds - 700.0).abs() < 0.001
    );
    assert_eq!(a["rankings"]["apps_by_network_out"][0]["app_pid"], 20);
    let state: Value =
        serde_json::from_slice(&fs::read(lab.0.join("nettop.json")).unwrap()).unwrap();
    let calls = state["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 2);
    for call in calls {
        let args = call.as_array().unwrap();
        for flag in ["-n", "-P", "-x"] {
            assert!(args.contains(&json!(flag)), "{call}");
        }
        assert!(
            args.windows(2)
                .any(|pair| pair == [json!("-J"), json!("bytes_in,bytes_out")])
        );
        assert!(
            !args.contains(&json!("-p")),
            "must include every user: {call}"
        );
        assert!(
            !args.contains(&json!("-d")),
            "must read cumulative counters: {call}"
        );
    }
}

#[test]
fn now_measures_the_window_and_ranks_root_memory_and_responsible_apps() {
    let mut before = row(100, 0, "20000:00.00", 10380902, "/usr/libexec/fseventsd");
    before += &row(101, 0, "0:00.00", 20, "/root/idle then busy");
    for pid in 200..231 {
        before += &row(pid, 501, "0:01.00", 138685, "/opt/claude");
    }
    let mut after = row(100, 0, "20000:00.00", 10380902, "/usr/libexec/fseventsd");
    after += &row(101, 0, "0:01.00", 20, "/root/idle then busy");
    for pid in 200..231 {
        after += &row(pid, 501, "0:01.01", 138685, "/opt/claude");
    }
    let lab = Lab::new(before, after);
    lab.process(100, 100);
    lab.process(101, 101);
    for pid in 200..231 {
        lab.process(pid, 200);
    }
    let a = lab.answer();
    assert_eq!(a["source"], "fixtures");
    assert_eq!(a["processes"].as_array().unwrap().len(), 33);
    assert_eq!(a["host"]["cpu"]["user_percent"], 30.0);
    assert_eq!(a["host"]["cpu"]["system_percent"], 10.0);
    assert_eq!(a["host"]["cpu"]["idle_percent"], 60.0);
    assert_eq!(a["host"]["memory"]["total_bytes"], 51539607552u64);
    assert_eq!(a["host"]["memory"]["pressure"], "warn");
    let memory = &a["rankings"]["processes_by_memory"];
    assert_eq!(memory[0]["name"], "fseventsd");
    assert_eq!(memory[0]["user"], "root");
    assert_eq!(memory[0]["resident_bytes"], 10630043648u64);
    assert_eq!(memory[0]["cpu_percent"], 0.0);
    assert_eq!(memory[0]["started_at"], "Mon Oct 12 17:34:33 2026");
    assert!(memory[0]["footprint_bytes"].is_null());
    assert!(memory[0]["peak_footprint_bytes"].is_null());
    let busy = &a["rankings"]["processes_by_cpu"][0];
    assert_eq!(busy["pid"], 101);
    let window = a["window_seconds"].as_f64().unwrap();
    assert!(window >= 1.0);
    assert!((busy["cpu_percent"].as_f64().unwrap() * window - 100.0).abs() < 0.01);
    let claude = &a["rankings"]["apps_by_memory"][1];
    assert_eq!(claude["name"], "claude");
    assert_eq!(claude["process_count"], 31);
    assert_eq!(claude["resident_bytes"], 4402416640u64);
    assert_eq!(claude["footprint_bytes"], 31000);
    assert_eq!(claude["peak_footprint_bytes"], 62000);
    for key in [
        "processes_by_cpu",
        "processes_by_memory",
        "apps_by_cpu",
        "apps_by_memory",
    ] {
        assert_eq!(a["rankings"][key].as_array().unwrap().len(), 2, "{key}");
    }
    assert_eq!(a["rankings"]["apps_by_cpu"][0]["app_pid"], 101);
    assert_eq!(a["rankings"]["apps_by_cpu"][1]["app_pid"], 200);
    assert!(
        (a["rankings"]["apps_by_cpu"][1]["cpu_percent"]
            .as_f64()
            .unwrap()
            * window
            - 31.0)
            .abs()
            < 0.01
    );
    let skipped = a["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1, "{skipped:?}");
    assert_eq!(skipped[0]["count"], 2);
    assert_eq!(skipped[0]["fix"], "sudo hostvitals now");
}

#[test]
fn now_rejects_invalid_limits_and_explains_every_answer_key() {
    for flag in ["--seconds", "--top"] {
        for value in ["0", "-1", "1.5", "abc", "+2", "18446744073709551616"] {
            let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
                .args(["now", flag, value])
                .output()
                .unwrap();
            assert!(!out.status.success());
            let answer: Value = serde_json::from_slice(&out.stdout).unwrap();
            let error = answer["error"].as_str().unwrap();
            assert!(error.contains("whole number at least 1"), "{error}");
            assert!(
                error.contains(&format!("hostvitals now {flag} 1")),
                "{error}"
            );
        }
    }
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .args(["now", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for key in [
        "source",
        "window_seconds",
        "host",
        "processes",
        "apps",
        "rankings",
        "skipped",
        "cpu",
        "user_percent",
        "system_percent",
        "idle_percent",
        "logical_cores",
        "physical_cores",
        "load_average",
        "memory",
        "total_bytes",
        "free_percent",
        "pressure",
        "swap_used_bytes",
        "swap_total_bytes",
        "compressed_bytes",
        "pid",
        "parent_pid",
        "uid",
        "user",
        "name",
        "started_at",
        "cpu_percent",
        "resident_bytes",
        "footprint_bytes",
        "peak_footprint_bytes",
        "app_pid",
        "process_count",
        "processes_by_cpu",
        "processes_by_memory",
        "apps_by_cpu",
        "apps_by_memory",
        "network",
        "bytes_in_per_second",
        "bytes_out_per_second",
        "processes_by_network_in",
        "processes_by_network_out",
        "apps_by_network_in",
        "apps_by_network_out",
        "measurement",
        "count",
        "reason",
        "fix",
    ] {
        assert!(help.contains(key), "missing {key}: {help}");
    }
}

#[test]
fn network_churn_and_regressions_are_null_and_do_not_form_partial_app_or_host_rates() {
    let before = row(10, 501, "0:00.00", 1, "/App")
        + &row(11, 501, "0:00.00", 1, "/reused")
        + &row(12, 501, "0:00.00", 1, "/regressed")
        + &row(13, 501, "0:00.00", 1, "/exited")
        + &row(15, 501, "0:00.00", 1, "/lost socket")
        + &row(16, 501, "0:00.00", 1, "/gained socket");
    let after = row(10, 501, "0:00.00", 1, "/App")
        + &row(11, 501, "0:00.00", 1, "/reused").replace("Oct 12", "Oct 25")
        + &row(12, 501, "0:00.00", 1, "/regressed")
        + &row(14, 501, "0:00.00", 1, "/new")
        + &row(15, 501, "0:00.00", 1, "/lost socket")
        + &row(16, 501, "0:00.00", 1, "/gained socket");
    let lab = Lab::new(before, after);
    for pid in [10, 11, 12, 14, 15, 16] {
        lab.process(pid, if pid == 12 { 12 } else { 10 });
    }
    lab.write("nettop", json!({"stdout_readings":[
        ",bytes_in,bytes_out,\nApp.10,100,200,\nreused.11,500,600,\nregressed.12,1000,1000,\nexited.13,50,60,\nlost.15,100,200,\n",
        ",bytes_in,bytes_out,\nApp.10,150,260,\nreused.11,700,800,\nregressed.12,500,1100,\nnew.14,100,200,\ngained.16,100,200,\n"
    ]}));
    let a = lab.answer();
    let processes = a["processes"].as_array().unwrap();
    for pid in [11, 14, 15, 16] {
        let p = processes.iter().find(|p| p["pid"] == pid).unwrap();
        assert!(p["network"]["bytes_in_per_second"].is_null(), "{p}");
        assert!(p["network"]["bytes_out_per_second"].is_null(), "{p}");
    }
    let regressed = processes.iter().find(|p| p["pid"] == 12).unwrap();
    assert!(regressed["network"]["bytes_in_per_second"].is_null());
    assert!(
        regressed["network"]["bytes_out_per_second"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    let app = a["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["app_pid"] == 10)
        .unwrap();
    assert!(app["network"]["bytes_in_per_second"].is_null());
    assert!(app["network"]["bytes_out_per_second"].is_null());
    assert_eq!(a["rankings"]["apps_by_network_in"], json!([]));
    assert_eq!(a["rankings"]["apps_by_network_out"][0]["app_pid"], 12);
    assert_eq!(
        a["rankings"]["processes_by_network_in"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(a["host"]["network"]["bytes_in_per_second"].is_null());
    assert!(a["host"]["network"]["bytes_out_per_second"].is_null());
    let skipped = a["skipped"].as_array().unwrap();
    assert!(
        skipped
            .iter()
            .any(|s| s["measurement"] == "network.bytes_in_per_second" && s["count"] == 5)
    );
    assert!(
        skipped
            .iter()
            .any(|s| s["measurement"] == "host.network.bytes_out_per_second" && s["count"] == 1)
    );
}

#[test]
fn network_failures_skip_only_network_and_never_accept_a_partial_or_invented_report() {
    for text in [
        "",
        "permission denied",
        ",bytes_in,bytes_out,\none.1,10,20,\nbroken\n",
        ",bytes_in,bytes_out,\none.1,10,20,\none.1,10,20,\n",
        ",bytes_in,bytes_out,\none.1,18446744073709551616,20,\n",
        ",bytes_in,bytes_out,\none.1,-1,20,\n",
        ",bytes_in,bytes_out,\none.1,+1,20,\n",
        ",bytes_in,bytes_out,\none.1,1 KiB,20,\n",
        ",bytes_in,bytes_out,\none.1,10,\n",
        ",bytes_in,bytes_out,\none.4294967296,10,20,\n",
        ",bytes_in,other,\none.1,10,20,\n",
    ] {
        let rows = row(1, 501, "0:00.00", 1, "/one");
        let lab = Lab::new(rows.clone(), rows);
        lab.process(1, 1);
        lab.write(
            "nettop",
            json!({"stdout_readings":[",bytes_in,bytes_out,\none.1,10,20,\n",text]}),
        );
        let a = lab.answer();
        assert!(a["host"]["network"]["window_seconds"].is_null(), "{text}");
        assert!(
            a["processes"][0]["network"]["bytes_in_per_second"].is_null(),
            "{text}"
        );
        assert_eq!(a["rankings"]["processes_by_network_in"], json!([]));
        assert_eq!(a["processes"][0]["cpu_percent"], 0.0);
        let skipped = a["skipped"].as_array().unwrap();
        let network = skipped
            .iter()
            .find(|s| s["measurement"] == "network")
            .unwrap();
        assert_eq!(network["count"], 2);
        assert!(network["reason"].as_str().unwrap().contains("nettop"));
        assert!(
            network["fix"]
                .as_str()
                .unwrap()
                .contains("hostvitals doctor")
        );
    }
    let rows = row(1, 501, "0:00.00", 1, "/one");
    let lab = Lab::new(rows.clone(), rows);
    lab.process(1, 1);
    lab.write(
        "nettop",
        json!({"exit_code":77,"stderr":"permission denied"}),
    );
    let a = lab.answer();
    assert!(
        a["skipped"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("permission denied")
    );
    fs::remove_file(lab.0.join("bin/nettop")).unwrap();
    lab.write("ps", json!({"stdout":row(1, 501, "0:00.00", 1, "/one")}));
    let a = lab.answer();
    assert!(a["skipped"][0]["fix"].as_str().unwrap().contains("PATH="));
    assert!(a["host"]["network"]["bytes_out_per_second"].is_null());
}

#[test]
fn network_uses_elapsed_time_handles_column_order_and_breaks_ties_by_pid() {
    let rows = row(1, 501, "0:00.00", 1, "/one") + &row(2, 501, "0:00.00", 1, "/two");
    let lab = Lab::new(rows.clone(), rows);
    lab.process(1, 1);
    lab.process(2, 2);
    lab.write("nettop", json!({"delay_ms":200,"stdout_readings":[
        ",bytes_out,bytes_in,\ntwo.2,18446744073709551610,100,\none.1,18446744073709551610,100,\n",
        ",bytes_in,bytes_out\none.1,400,18446744073709551615\ntwo.2,400,18446744073709551615\n"
    ]}));
    let a = lab.answer();
    let seconds = a["host"]["network"]["window_seconds"].as_f64().unwrap();
    assert!(seconds >= 1.2);
    assert!(
        (a["host"]["network"]["bytes_in_per_second"]
            .as_f64()
            .unwrap()
            * seconds
            - 600.0)
            .abs()
            < 0.001
    );
    assert!(
        (a["host"]["network"]["bytes_out_per_second"]
            .as_f64()
            .unwrap()
            * seconds
            - 10.0)
            .abs()
            < 0.001
    );
    for key in ["processes_by_network_in", "processes_by_network_out"] {
        assert_eq!(a["rankings"][key][0]["pid"], 1);
        assert_eq!(a["rankings"][key][1]["pid"], 2);
    }
    for key in ["apps_by_network_in", "apps_by_network_out"] {
        assert_eq!(a["rankings"][key][0]["app_pid"], 1);
        assert_eq!(a["rankings"][key][1]["app_pid"], 2);
    }
}

#[test]
fn network_host_total_overflow_skips_only_the_affected_direction() {
    let rows = row(1, 501, "0:00.00", 1, "/one") + &row(2, 501, "0:00.00", 1, "/two");
    let lab = Lab::new(rows.clone(), rows);
    lab.process(1, 1);
    lab.process(2, 2);
    lab.write(
        "nettop",
        json!({"stdout_readings":[
            ",bytes_in,bytes_out,\none.1,0,0,\ntwo.2,0,0,\n",
            ",bytes_in,bytes_out,\none.1,18446744073709551615,10,\ntwo.2,18446744073709551615,20,\n"
        ]}),
    );
    let a = lab.answer();
    let seconds = a["host"]["network"]["window_seconds"].as_f64().unwrap();
    assert!(a["host"]["network"]["bytes_in_per_second"].is_null());
    assert!(
        (a["host"]["network"]["bytes_out_per_second"]
            .as_f64()
            .unwrap()
            * seconds
            - 30.0)
            .abs()
            < 0.001
    );
    assert_eq!(
        a["rankings"]["processes_by_network_in"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        a["rankings"]["apps_by_network_in"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let skipped = a["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(
        skipped[0]["measurement"],
        "host.network.bytes_in_per_second"
    );
}

#[test]
fn churn_and_missing_native_reads_remain_explicit_and_never_become_zeroes() {
    let before = row(10, 501, "1-00:00:00.00", 100, "/App Parent")
        + &row(11, 501, "10:00:00.00", 200, "/child")
        + &row(12, 501, "1:00.00", 300, "/regressed")
        + &row(13, 501, "0:00.00", 400, "/exited");
    let after = row(10, 501, "1-00:00:02.00", 100, "/App Parent")
        + &row(11, 501, "0:00.00", 200, "/replacement").replace("Oct 12", "Oct 25")
        + &row(12, 501, "0:00.00", 300, "/regressed")
        + &row(14, 501, "0:00.00", 500, "/new")
        + &row(15, 501, "0:00.00", 100, "/no app");
    let lab = Lab::new(before, after);
    for pid in [10, 11, 12, 14, 15] {
        lab.process(pid, 10);
    }
    fs::remove_file(lab.0.join("now-rusage-12.json")).unwrap();
    lab.write(
        "now-responsibility-15",
        json!({"status":"unavailable","reason":"symbol removed"}),
    );
    let a = lab.answer();
    let processes = a["processes"].as_array().unwrap();
    assert_eq!(processes.len(), 5);
    assert!(processes.iter().all(|p| p["pid"] != 13));
    for pid in [11, 12, 14, 15] {
        let p = processes.iter().find(|p| p["pid"] == pid).unwrap();
        assert!(p["cpu_percent"].is_null(), "{p}");
    }
    assert!(processes.iter().find(|p| p["pid"] == 12).unwrap()["footprint_bytes"].is_null());
    assert!(processes.iter().find(|p| p["pid"] == 15).unwrap()["app_pid"].is_null());
    assert_eq!(
        a["rankings"]["processes_by_cpu"].as_array().unwrap().len(),
        1
    );
    assert_eq!(a["rankings"]["processes_by_cpu"][0]["pid"], 10);
    assert!(
        a["rankings"]["processes_by_cpu"][0]["cpu_percent"]
            .as_f64()
            .unwrap()
            > 100.0
    );
    assert_eq!(a["apps"][0]["process_count"], 4);
    assert!(a["apps"][0]["cpu_percent"].is_null());
    assert!(a["apps"][0]["footprint_bytes"].is_null());
    assert_eq!(a["apps"][0]["resident_bytes"], 1126400);
    assert_eq!(a["rankings"]["apps_by_cpu"], json!([]));
    let skipped = a["skipped"].as_array().unwrap();
    assert!(
        skipped
            .iter()
            .any(|s| s["measurement"] == "cpu_percent" && s["count"] == 4)
    );
    assert!(
        skipped
            .iter()
            .any(|s| s["measurement"] == "app_pid" && s["reason"] == "symbol removed")
    );
}

#[test]
fn root_reads_other_users_footprints_and_ties_are_ordered_by_pid() {
    let rows = row(2, 0, "0:01.00", 20, "/two") + &row(1, 501, "0:01.00", 20, "/one");
    let lab = Lab::new(rows.clone(), rows);
    lab.process(1, 88);
    lab.process(2, 88);
    for i in 0..2 {
        let path = lab.0.join(format!("now-host-{i}.json"));
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["data"]["euid"] = json!(0);
        fs::write(path, value.to_string()).unwrap();
    }
    let a = lab.answer();
    assert_eq!(a["skipped"], json!([]));
    assert_eq!(a["rankings"]["processes_by_cpu"][0]["pid"], 1);
    assert_eq!(a["rankings"]["processes_by_memory"][0]["pid"], 1);
    assert_eq!(a["apps"][0]["footprint_bytes"], 2000);
    assert_eq!(a["apps"][0]["app_pid"], 88);
    assert!(a["apps"][0]["name"].is_null());
    assert!(a["apps"][0]["started_at"].is_null());
}

#[test]
fn malformed_or_failed_sources_refuse_instead_of_returning_partial_inventory() {
    for text in [
        "",
        "oops\n",
        "1 1 501 me bad-cpu 20 Mon Oct 12 00:00:00 2026 /bad\n",
    ] {
        let lab = Lab::new(text.into(), text.into());
        let out = lab.run(&["now", "--seconds", "1"]);
        assert!(!out.status.success());
        let a: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(a["error"].as_str().unwrap().contains("hostvitals doctor"));
    }
    let rows = row(1, 501, "0:00.00", 1, "/one");
    let lab = Lab::new(rows.clone(), rows);
    lab.write("ps", json!({"exit_code":77,"stderr":"permission denied"}));
    let out = lab.run(&["now", "--seconds", "1"]);
    assert!(!out.status.success());
    let a: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(a["error"].as_str().unwrap().contains("permission denied"));
    fs::remove_file(lab.0.join("bin/ps")).unwrap();
    let out = lab.run(&["now"]);
    let a: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(a["error"].as_str().unwrap().contains("PATH="));
}

#[test]
fn host_fixture_failure_never_falls_back_and_tick_wrap_includes_nice_time() {
    let rows = row(1, 501, "0:00.00", 1, "/one");
    let lab = Lab::new(rows.clone(), rows);
    lab.process(1, 1);
    for (i, ticks) in [[4294967290u32, 0, 0, 0], [4, 10, 60, 20]]
        .into_iter()
        .enumerate()
    {
        let path = lab.0.join(format!("now-host-{i}.json"));
        let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["data"]["cpu_ticks"] = json!(ticks);
        fs::write(path, value.to_string()).unwrap();
    }
    let a = lab.answer();
    assert_eq!(a["host"]["cpu"]["user_percent"], 30.0);
    assert_eq!(a["host"]["cpu"]["system_percent"], 10.0);
    assert_eq!(a["host"]["cpu"]["idle_percent"], 60.0);
    // Reset the CLI state before a separate invocation.
    lab.write(
        "ps",
        json!({"stdout_readings":[row(1,501,"0:00.00",1,"/one")]}),
    );
    fs::remove_file(lab.0.join("now-host-0.json")).unwrap();
    let out = lab.run(&["now", "--seconds", "1"]);
    assert!(!out.status.success());
    let a: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(a["error"].as_str().unwrap().contains("fixture now-host-0"));
}

#[test]
fn live_now_reads_host_and_every_user_without_root() {
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .args(["now", "--seconds", "1", "--top", "3"])
        .env_remove("HOSTVITALS_PROBE_FIXTURES")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let a: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(a["source"], "host");
    assert!(a["host"]["network"]["window_seconds"].as_f64().unwrap() >= 1.0);
    let network_processes = a["processes"].as_array().unwrap();
    for direction in ["bytes_in_per_second", "bytes_out_per_second"] {
        assert!(
            network_processes.iter().any(
                |p| p["uid"] == 0 && p["network"][direction].as_f64().is_some_and(|n| n >= 0.0)
            ),
            "root network unreadable: {a}"
        );
        for p in network_processes {
            if let Some(rate) = p["network"][direction].as_f64() {
                assert!(rate.is_finite() && rate >= 0.0);
            }
        }
    }
    let cpu = &a["host"]["cpu"];
    let mut total = 0.0;
    for key in ["user_percent", "system_percent", "idle_percent"] {
        let percent = cpu[key].as_f64().unwrap();
        assert!((0.0..=100.0).contains(&percent));
        total += percent;
    }
    assert!((total - 100.0).abs() < 0.001);
    assert!(cpu["logical_cores"].as_u64().unwrap() >= cpu["physical_cores"].as_u64().unwrap());
    assert!(a["host"]["memory"]["total_bytes"].as_u64().unwrap() > 0);
    assert!(
        ["normal", "warn", "critical"].contains(&a["host"]["memory"]["pressure"].as_str().unwrap())
    );
    assert!(
        a["host"]["memory"]["swap_used_bytes"].as_u64().unwrap()
            <= a["host"]["memory"]["swap_total_bytes"].as_u64().unwrap()
    );
    let processes = a["processes"].as_array().unwrap();
    assert!(
        processes
            .iter()
            .any(|p| p["uid"] == 0 && p["resident_bytes"].as_u64().unwrap() > 0)
    );
    assert!(
        processes
            .iter()
            .any(|p| p["name"] == "hostvitals"
                && p["footprint_bytes"].as_u64().is_some_and(|n| n > 0))
    );
    assert!(
        processes
            .iter()
            .all(|p| p["started_at"].as_str().is_some_and(|s| !s.is_empty()))
    );
    assert_eq!(
        a["rankings"]["processes_by_memory"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}
