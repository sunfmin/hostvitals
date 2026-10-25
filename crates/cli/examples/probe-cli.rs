//! A stand-in for each Apple CLI, selected by argv[0], with one JSON state file per tool.
use hostvitals::tools::{StandIn, Tool};
use std::{env, fs, path::Path};
fn main() {
    let args: Vec<_> = env::args().collect();
    let name = Path::new(&args[0]).file_name().unwrap().to_str().unwrap();
    let Some(tool) = Tool::all().iter().find(|tool| tool.name() == name) else {
        return;
    };
    let dir = env::var_os("HOSTVITALS_PROBE_FIXTURES").expect("fixture directory");
    let path = Path::new(&dir).join(format!("{}.json", tool.name()));
    let mut state: StandIn = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    state.calls.push(args[1..].to_vec());
    fs::write(path, serde_json::to_vec(&state).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(state.delay_ms));
    eprint!("{}", state.stderr);
    print!("{}", state.stdout);
    std::process::exit(state.exit_code);
}
