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
            "hostvitals-process-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(dir.join("bin")).unwrap();
        let lab = Self(dir);
        let standin = PathBuf::from(env!("CARGO_BIN_EXE_hostvitals"))
            .parent()
            .unwrap()
            .join("examples/probe-cli");
        for tool in ["ps", "lsof", "dscacheutil", "fs_usage"] {
            std::os::unix::fs::symlink(&standin, lab.0.join("bin").join(tool)).unwrap();
        }
        lab.write(
            "ps",
            json!({"stdout_readings":[row("0:01.00"),row("0:03.00")]}),
        );
        lab.write("process-owner", json!({"status":"available","data":501}));
        for (i, read, written) in [(0, 100, 200), (1, 400, 700)] {
            lab.write(&format!("process-rusage-{i}"), json!({"status":"available","data":{"resident_bytes":4096,"footprint_bytes":8192,"peak_footprint_bytes":16384,"read_bytes":read,"written_bytes":written,"energy_nj":1234}}));
        }
        lab.write(
            "process-task",
            json!({"status":"available","data":{"pid":123,"resident_bytes":4096,"threads":7}}),
        );
        lab.write(
            "process-app",
            json!({"status":"available","data":{"pid":123,"app_pid":123}}),
        );
        lab.write("lsof", json!({"stdout":"p123\0\nfcwd\0tDIR\0n/tmp/work space\0\nf4\0tREG\0n/tmp/open file\0\nf5\0tIPv4\0PTCP\0n127.0.0.1:5000->192.0.2.1:443\0TST=ESTABLISHED\0\nf6\0tIPv6\0PUDP\0n[::1]:5001->[2001:db8::1]:53\0\nf7\0tIPv4\0PTCP\0n*:8080\0TST=LISTEN\0\nf8\0tunix\0n/tmp/unix socket\0\n"}));
        lab.write(
            "dscacheutil",
            json!({"stdout":"name: remote.example\nip_address: 192.0.2.1\n"}),
        );
        lab.write("fs_usage", json!({"stdout":"12:00:00.000001  read F=4 B=0x10 /tmp/read file 0.000010 W app.123\n12:00:00.000002  write F=4 B=0x20 /tmp/written file 0.000020 app.123\n"}));
        lab.arguments(&[
            "program",
            "--password",
            "private two words",
            "--label",
            "public two words",
            "https://alice:privatepass@host/path",
            "",
            "--token=privatevalue",
        ]);
        lab
    }
    fn write(&self, name: &str, value: Value) {
        fs::write(self.0.join(format!("{name}.json")), value.to_string()).unwrap();
    }
    fn arguments(&self, arguments: &[&str]) {
        let mut bytes = (arguments.len() as i32).to_ne_bytes().to_vec();
        // XNU pads the executable path to an eight-byte boundary, excluding argc.
        bytes.extend_from_slice(b"/bin/program\0\0\0\0");
        for arg in arguments {
            bytes.extend_from_slice(arg.as_bytes());
            bytes.push(0);
        }
        bytes.extend_from_slice(b"ENVIRONMENT_PRIVATE_SENTINEL=never_export_me\0");
        fs::write(self.0.join("process-arguments.bin"), bytes).unwrap();
    }
    fn run(&self) -> (std::process::Output, Value) {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(["process", "123", "--seconds", "1"])
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
fn row(cpu: &str) -> String {
    format!("123 12 501 me {cpu} 4 Mon Oct 12 00:00:00 2026 /bin/program\n")
}

#[test]
fn process_answers_identity_counters_files_sockets_and_only_redacted_arguments() {
    let lab = Lab::new();
    let (out, a) = lab.run();
    assert!(out.status.success(), "{a}");
    assert_eq!(a["source"], "fixtures");
    assert_eq!(a["pid"], 123);
    assert_eq!(a["parent_pid"], 12);
    assert_eq!(a["user"], "me");
    assert_eq!(a["app"]["app_pid"], 123);
    assert_eq!(
        a["command_line"],
        json!([
            "program",
            "--password",
            "<redacted>",
            "--label",
            "public two words",
            "https://<redacted>@host/path",
            "",
            "--token=<redacted>"
        ])
    );
    assert_eq!(a["cwd"], "/tmp/work space");
    assert_eq!(a["cpu_seconds"], 3.0);
    let window = a["window_seconds"].as_f64().unwrap();
    assert!(window >= 1.0);
    assert!((a["cpu_percent"].as_f64().unwrap() * window - 200.0).abs() < 0.001);
    assert_eq!(a["footprint_bytes"], 8192);
    assert_eq!(a["peak_footprint_bytes"], 16384);
    assert_eq!(a["disk"]["read_bytes"], 400);
    assert_eq!(a["disk"]["window_read_bytes"], 300);
    assert_eq!(a["disk"]["window_written_bytes"], 500);
    assert!(a["disk"]["window_seconds"].as_f64().unwrap() >= 1.0);
    assert_eq!(a["energy_nj"], 1234);
    assert_eq!(a["threads"], 7);
    assert_eq!(a["open_files"][0]["path"], "/tmp/open file");
    assert_eq!(a["sockets"][0]["remote_ip"], "192.0.2.1");
    assert_eq!(a["sockets"][0]["remote_port"], 443);
    assert_eq!(a["sockets"][0]["remote_name"], "remote.example");
    assert_eq!(a["sockets"][1]["remote_ip"], "2001:db8::1");
    assert!(a["sockets"][2]["remote_ip"].is_null());
    assert_eq!(a["sockets"][3]["local"], "/tmp/unix socket");
    assert!(a["file_activity"].is_null());
    assert_eq!(a["skipped"][0]["measurement"], "file_activity");
    for secret in [
        "private two words",
        "privatepass",
        "privatevalue",
        "alice",
        "ENVIRONMENT_PRIVATE_SENTINEL",
        "never_export_me",
    ] {
        assert!(!String::from_utf8_lossy(&out.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&out.stderr).contains(secret));
    }
}

#[test]
fn other_users_are_ps_only_and_every_skip_gives_the_root_line() {
    let lab = Lab::new();
    lab.write("process-owner", json!({"status":"available","data":999}));
    let (out, a) = lab.run();
    assert!(out.status.success(), "{a}");
    assert_eq!(a["resident_bytes"], 4096);
    assert_eq!(a["cpu_seconds"], 3.0);
    for field in [
        "app",
        "command_line",
        "cwd",
        "footprint_bytes",
        "peak_footprint_bytes",
        "energy_nj",
        "threads",
        "open_files",
        "sockets",
        "file_activity",
    ] {
        assert!(a[field].is_null(), "{field}: {a}");
        assert!(
            a["skipped"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["measurement"] == field
                    && s["fix"] == "sudo hostvitals process 123 --seconds 1")
        );
    }
    assert!(
        !lab.0.join("lsof.json").is_file()
            || serde_json::from_slice::<Value>(&fs::read(lab.0.join("lsof.json")).unwrap())
                .unwrap()["calls"]
                .is_null()
    );
    assert!(a["disk"]["read_bytes"].is_null());
}

#[test]
fn root_collects_read_and_write_paths_with_spaces_and_exact_requested_bytes() {
    let lab = Lab::new();
    lab.write("process-owner", json!({"status":"available","data":0}));
    let (out, a) = lab.run();
    assert!(out.status.success(), "{a}");
    assert_eq!(
        a["file_activity"],
        json!([
            {"operation":"read","path":"/tmp/read file","bytes":16},
            {"operation":"write","path":"/tmp/written file","bytes":32}
        ])
    );
    assert_eq!(a["skipped"], json!([]));
}

#[test]
fn all_names_share_two_seconds_and_failed_lookups_leave_literal_ips() {
    let lab = Lab::new();
    lab.write(
        "dscacheutil",
        json!({"delay_ms":5000,"stdout":"name: too-late.example\n"}),
    );
    let start = std::time::Instant::now();
    let (out, a) = lab.run();
    assert!(out.status.success(), "{a}");
    assert!(
        start.elapsed().as_secs_f64() < 4.5,
        "deadline was per socket"
    );
    for i in 0..2 {
        assert_eq!(a["sockets"][i]["remote_name"], a["sockets"][i]["remote_ip"]);
    }
    let lab = Lab::new();
    lab.write(
        "dscacheutil",
        json!({"exit_code":1,"stderr":"lookup failed"}),
    );
    let (out, a) = lab.run();
    assert!(out.status.success());
    for i in 0..2 {
        assert_eq!(a["sockets"][i]["remote_name"], a["sockets"][i]["remote_ip"]);
    }
}

#[test]
fn malformed_arguments_never_disclose_environment_in_output_diagnostics_or_error() {
    let lab = Lab::new();
    let mut bytes = 0i32.to_ne_bytes().to_vec();
    bytes.extend_from_slice(b"ENVIRONMENT_PRIVATE_SENTINEL=never_export_me\0");
    fs::write(lab.0.join("process-arguments.bin"), bytes).unwrap();
    let (out, a) = lab.run();
    assert!(out.status.success());
    assert!(a["command_line"].is_null());
    assert!(
        a["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["measurement"] == "command_line")
    );
    for stream in [&out.stdout, &out.stderr] {
        assert!(!String::from_utf8_lossy(stream).contains("ENVIRONMENT_PRIVATE_SENTINEL"));
        assert!(!String::from_utf8_lossy(stream).contains("never_export_me"));
    }
}

#[test]
fn missing_exited_and_reused_pids_refuse_with_the_pid() {
    for readings in [
        vec![row("0:01.00").replace("123 ", "124 ")],
        vec![row("0:01.00"), row("0:01.00").replace("123 ", "124 ")],
        vec![row("0:01.00"), row("0:00.00").replace("Oct 12", "Oct 26")],
    ] {
        let lab = Lab::new();
        lab.write("ps", json!({"stdout_readings":readings}));
        let (out, a) = lab.run();
        assert!(!out.status.success(), "{a}");
        assert_eq!(a.as_object().unwrap().len(), 1);
        assert!(a["error"].as_str().unwrap().contains("123"));
    }
}

#[test]
fn missing_denied_and_regressed_reads_are_explicit_skips() {
    let lab = Lab::new();
    lab.write("process-rusage-0",json!({"status":"available","data":{"resident_bytes":4096,"footprint_bytes":8192,"peak_footprint_bytes":16384,"read_bytes":1000,"written_bytes":2000,"energy_nj":1234}}));
    lab.write("lsof", json!({"exit_code":1,"stderr":"permission denied"}));
    fs::remove_file(lab.0.join("process-task.json")).unwrap();
    let (out, a) = lab.run();
    assert!(out.status.success(), "{a}");
    assert_eq!(a["disk"]["read_bytes"], 400);
    assert!(a["disk"]["window_read_bytes"].is_null());
    assert!(a["disk"]["window_written_bytes"].is_null());
    for field in ["threads", "cwd", "open_files", "sockets"] {
        assert!(a[field].is_null(), "{field}");
    }
    assert!(
        a["skipped"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["measurement"] == "disk.window_bytes")
    );
}

#[test]
fn help_names_every_key_and_invalid_requests_are_json_errors() {
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .args(["process", "--help"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for key in [
        "source",
        "pid",
        "parent_pid",
        "uid",
        "user",
        "name",
        "started_at",
        "app",
        "app_pid",
        "command_line",
        "cwd",
        "cpu_seconds",
        "cpu_percent",
        "window_seconds",
        "resident_bytes",
        "footprint_bytes",
        "peak_footprint_bytes",
        "disk",
        "read_bytes",
        "written_bytes",
        "window_read_bytes",
        "window_written_bytes",
        "energy_nj",
        "threads",
        "open_files",
        "fd",
        "kind",
        "path",
        "sockets",
        "protocol",
        "local",
        "remote_ip",
        "remote_port",
        "remote_name",
        "state",
        "file_activity",
        "operation",
        "bytes",
        "skipped",
        "measurement",
        "reason",
        "fix",
    ] {
        assert!(help.contains(key), "missing {key}");
    }
    for args in [
        vec!["process"],
        vec!["process", "0"],
        vec!["process", "2147483648"],
        vec!["process", "abc"],
        vec!["process", "1", "--seconds", "0"],
        vec!["process", "1", "--seconds", "-1"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
            .args(args)
            .output()
            .unwrap();
        assert!(!out.status.success());
        let a: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(a["error"].as_str().unwrap().contains("hostvitals --help"));
    }
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn live_native_argv_keeps_boundaries_withholds_environment_and_reads_real_open_file() {
    let lab = Lab::new();
    let file = lab.0.join("a live open file");
    fs::write(&file, "public contents").unwrap();
    let child = Child(
        Command::new("/bin/sh")
            .args(["-c", "exec 9<\"$1\"; while :; do :; done", "live-subject"])
            .arg(&file)
            .args([
                "--PaSSword",
                "PRIVATE multiword 💡 secret",
                "--token=PRIVATE_EQ_SECRET",
                "--auth",
                "--PRIVATE_FLAG_VALUE",
                "--key",
                "",
                "--label",
                "public two words",
                "https://PRIVATE_USER:PRIVATE_PASS@example.test/path",
                "",
            ])
            .env(
                "HOSTVITALS_ENVIRONMENT_SENTINEL",
                "PRIVATE_ENVIRONMENT_VALUE",
            )
            .current_dir(&lab.0)
            .spawn()
            .unwrap(),
    );
    std::thread::sleep(std::time::Duration::from_millis(50));
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .args(["process", &child.0.id().to_string(), "--seconds", "1"])
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
    let args = a["command_line"].as_array().unwrap();
    assert!(args.iter().any(|a| a == "public two words"));
    assert!(args.iter().any(|a| a == ""));
    assert!(args.iter().any(|a| a == "--token=<redacted>"));
    assert!(
        args.iter()
            .any(|a| a == "https://<redacted>@example.test/path")
    );
    for secret in [
        "PRIVATE multiword",
        "PRIVATE_EQ_SECRET",
        "PRIVATE_FLAG_VALUE",
        "PRIVATE_USER",
        "PRIVATE_PASS",
        "HOSTVITALS_ENVIRONMENT_SENTINEL",
        "PRIVATE_ENVIRONMENT_VALUE",
    ] {
        assert!(
            !String::from_utf8_lossy(&out.stdout).contains(secret),
            "{secret}"
        );
        assert!(
            !String::from_utf8_lossy(&out.stderr).contains(secret),
            "{secret}"
        );
    }
    assert!(a["cpu_percent"].as_f64().unwrap() > 0.0);
    assert!(a["threads"].as_u64().unwrap() > 0);
    assert!(a["footprint_bytes"].as_u64().unwrap() > 0);
    assert!(
        a["open_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == fs::canonicalize(&file).unwrap().to_str().unwrap())
    );
    assert_eq!(
        fs::canonicalize(a["cwd"].as_str().unwrap()).unwrap(),
        fs::canonicalize(&lab.0).unwrap()
    );
}

#[test]
fn live_sockets_include_connected_ipv4_and_listening_ipv6_without_losing_ports() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let _client = std::net::TcpStream::connect(address).unwrap();
    let (_server, _) = listener.accept().unwrap();
    let v6 = std::net::TcpListener::bind("[::1]:0").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_hostvitals"))
        .args(["process", &std::process::id().to_string(), "--seconds", "1"])
        .env_remove("HOSTVITALS_PROBE_FIXTURES")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let a: Value = serde_json::from_slice(&out.stdout).unwrap();
    let sockets = a["sockets"].as_array().unwrap();
    assert!(
        sockets
            .iter()
            .any(|s| s["remote_ip"] == "127.0.0.1" && s["remote_port"] == address.port()),
        "{sockets:?}"
    );
    assert!(
        sockets.iter().any(|s| s["kind"] == "IPv6"
            && s["local"]
                .as_str()
                .unwrap()
                .contains(&v6.local_addr().unwrap().port().to_string())),
        "{sockets:?}"
    );
}

#[test]
fn collector_warnings_and_malformed_files_never_become_complete_lists() {
    let lab = Lab::new();
    lab.write("process-owner", json!({"status":"available","data":0}));
    let mut lsof: Value =
        serde_json::from_slice(&fs::read(lab.0.join("lsof.json")).unwrap()).unwrap();
    lsof["stderr"] = json!("warning: cannot examine a file");
    lab.write("lsof", lsof);
    lab.write("fs_usage", json!({"stderr":"events were lost"}));
    let (out, a) = lab.run();
    assert!(out.status.success());
    assert!(a["open_files"].is_null(), "{a}");
    assert!(a["sockets"].is_null());
    assert!(a["file_activity"].is_null());
    let lab = Lab::new();
    lab.write("lsof", json!({"stdout":"p123\0\nf4\0tREG\0\n"}));
    let (out, a) = lab.run();
    assert!(out.status.success());
    assert!(a["open_files"].is_null());
}

#[test]
fn filesystem_activity_resolves_initial_and_new_descriptors_and_forgets_closed_ones() {
    let lab = Lab::new();
    lab.write("process-owner", json!({"status":"available","data":0}));
    lab.write("fs_usage",json!({"stdout":"12:00:00.000001 read F=4 B=0x10 0.000001 app.123\n12:00:00.000002 open F=9 /tmp/new file 0.000001 app.123\n12:00:00.000003 write F=9 B=0x20 0.000001 app.123\n12:00:00.000004 close F=9 0.000001 app.123\n12:00:00.000005 read F=9 B=0x10 0.000001 app.123\n"}));
    let (out, a) = lab.run();
    assert!(out.status.success(), "{a}");
    assert_eq!(a["file_activity"][0]["path"], "/tmp/open file");
    assert_eq!(a["file_activity"][1]["path"], "/tmp/new file");
    assert!(a["file_activity"][2]["path"].is_null());
}

#[test]
fn malformed_trace_diagnostics_and_scoped_ipv6_are_handled_without_invented_readings() {
    let lab = Lab::new();
    lab.write("process-owner", json!({"status":"available","data":0}));
    lab.write(
        "fs_usage",
        json!({"stdout":"fs_usage: tracing is unavailable\n"}),
    );
    lab.write("lsof",json!({"stdout":"p123\0\nfcwd\0tDIR\0n/tmp\0\nf5\0tIPv6\0PUDP\0n[fe80::1%en0]:5000->[fe80::2%en0]:5353\0\n"}));
    let (out, a) = lab.run();
    assert!(out.status.success());
    assert!(a["file_activity"].is_null(), "{a}");
    assert_eq!(a["sockets"][0]["remote_ip"], "fe80::2%en0");
    assert_eq!(a["sockets"][0]["remote_port"], 5353);
}

#[test]
fn live_empty_argv_zero_never_causes_environment_to_be_parsed_as_an_argument() {
    let child=Child(Command::new("/bin/bash").args(["-c","exec -a '' /bin/sh -c 'while :; do :; done'"])
        .env_clear().env("PRIVATE_ENV_SENTINEL","PRIVATE_ENV_VALUE").spawn().unwrap());
    std::thread::sleep(std::time::Duration::from_millis(50));
    let out=Command::new(env!("CARGO_BIN_EXE_hostvitals")).args(["process",&child.0.id().to_string(),"--seconds","1"])
        .env_remove("HOSTVITALS_PROBE_FIXTURES").output().unwrap();
    assert!(out.status.success());
    for bytes in [&out.stdout,&out.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains("PRIVATE_ENV_SENTINEL"));
        assert!(!String::from_utf8_lossy(bytes).contains("PRIVATE_ENV_VALUE"));
    }
    let a: Value=serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(a["command_line"][0],"");
    assert_eq!(a["command_line"][1],"-c");
    assert_eq!(a["command_line"][2],"while :; do :; done");
}
