//! Apple CLI stand-ins and a sleeping live Profile target, selected by argv[0].
use hostvitals::tools::{StandIn, Tool};
use std::{
    env, fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};
fn main() {
    let args: Vec<_> = env::args().collect();
    let name = Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
    if name == "profile-target" {
        let dir = Path::new(&args[1]);
        fs::write(dir.join("target-ready"), "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !dir.join("target-release").exists() {
            assert!(Instant::now() < deadline, "profile target was not released");
            thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    let Some(tool) = Tool::all().iter().find(|tool| tool.name() == name) else {
        return;
    };
    let dir = env::var_os("HOSTVITALS_PROBE_FIXTURES").expect("fixture directory");
    let path = Path::new(&dir).join(format!("{}.json", tool.name()));
    let mut state: StandIn = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state.calls.push(args[1..].to_vec());
    let stdout = state
        .stdout_readings
        .get(state.calls.len() - 1)
        .cloned()
        .unwrap_or_else(|| state.stdout.clone());
    fs::write(path, serde_json::to_vec(&state).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(state.delay_ms));
    if matches!(tool, Tool::Sample)
        && let Some(flag) = args
            .iter()
            .position(|arg| arg == hostvitals::tools::sample::REPORT_FLAG)
        && let Some(destination) = args.get(flag + 1)
        && destination != "/dev/null"
    {
        fs::write(destination, state.report.as_deref().unwrap_or(&stdout)).unwrap();
    }
    eprint!("{}", state.stderr);
    print!("{stdout}");
    std::process::exit(state.exit_code);
}
