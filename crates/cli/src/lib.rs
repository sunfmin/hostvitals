//! Read-only host observations exposed as a single JSON answer.
#[cfg(not(all(
    target_os = "macos",
    any(target_arch = "aarch64", target_arch = "x86_64")
)))]
compile_error!("hostvitals supports macOS arm64 and x86_64 only");
mod direct;
mod doctor;
mod help;
mod memory;
mod now;
pub mod tools;

use clap::FromArgMatches;
use std::{ffi::OsString, process::ExitCode};

/// Runs one request; all refusals are JSON on stdout with a nonzero exit code.
pub fn main(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let manual = help::manual();
    let command = match manual.clap() {
        Ok(command) => command,
        Err(e) => return refuse(e.to_string()),
    };
    let cli = match command
        .try_get_matches_from(args)
        .and_then(|matches| help::Cli::from_arg_matches(&matches))
    {
        Ok(cli) => cli,
        Err(e)
            if matches!(
                e.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            print!("{e}");
            return ExitCode::SUCCESS;
        }
        Err(e) => return refuse(format!("{e}; run hostvitals --help")),
    };
    match cli.command {
        help::Command::Now { seconds, top } => {
            let source =
                direct::Source(std::env::var_os("HOSTVITALS_PROBE_FIXTURES").map(Into::into));
            if source.0.is_some() {
                eprintln!("warning: native observations use explicit JSON fixtures");
            }
            match now::answer(&source, seconds, top) {
                Ok(answer) => println!("{answer}"),
                Err(error) => return refuse(error),
            }
        }
        help::Command::Memory { pid, top } => println!("{}", memory::answer(pid, top)),
        help::Command::Doctor => {
            let source =
                direct::Source(std::env::var_os("HOSTVITALS_PROBE_FIXTURES").map(Into::into));
            if source.0.is_some() {
                eprintln!("warning: native probes use explicit JSON fixtures");
            }
            match doctor::answer(&source) {
                Ok(answer) => println!("{answer}"),
                Err(error) => return refuse(error),
            }
        }
        help::Command::Help { topic } => {
            let markdown = match topic.as_deref() {
                None => manual.index(),
                Some("all") => manual.manual(),
                Some(topic) => manual.command_help(&[topic]),
            };
            match markdown {
                Ok(text) => print!("{text}"),
                Err(e) => return refuse(format!("{e}; run hostvitals --help")),
            }
        }
    }
    ExitCode::SUCCESS
}
fn refuse(error: String) -> ExitCode {
    println!("{}", serde_json::json!({"error":error}));
    ExitCode::FAILURE
}
