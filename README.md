# hostvitals

A read-only macOS agent CLI for host and process observations. Start with `hostvitals --help`, then `hostvitals doctor`; `hostvitals help all` opens the complete manual. The initial command inventories the observation capabilities described in the glossary and decisions.

Install on macOS arm64 or x86_64:

```sh
brew install sunfmin/tap/hostvitals
```

For an AI agent, install the skill that points to the tool's help:

```sh
npx skills add sunfmin/hostvitals
```

Some measurements need root and are skipped without it. hostvitals never runs `sudo` itself; `hostvitals doctor` reports the observation capabilities here and the fixes for anything missing.

Build locally with `cargo build --locked -p hostvitals` on macOS arm64 or x86_64. Stable Rust builds the binary; the pinned nightly runs the verification gate. `./gate.sh` is the same gate locally and in CI: format, Flux alone (no trusted or ignored functions), clippy, then nextest. The checker versions and nightly live in `Cargo.toml`; `./verify.sh` rejects mismatches with installation instructions. Install nextest with `brew install cargo-nextest`.

The effect crate owns native reads, CLI execution and JSON. The policy crate has no I/O and every function passes Flux. Real-binary integration tests substitute a cargo example for each Apple CLI on PATH. Build that stand-in with `cargo build --example probe-cli` before running an individual integration test; the gate builds it automatically.

For deterministic native reads, set `HOSTVITALS_PROBE_FIXTURES` to a directory containing `host.json`, `rusage.json`, `libproc.json`, `ioreport.json` and `responsibility.json`. The host fixture has `macos_version`, `arch` and numeric `euid`. Each native fixture is `{"status":"available","data":{...}}` with the fields documented by `hostvitals doctor --help`, or `{"status":"unavailable","reason":"..."}`. Doctor warns on stderr and labels the answer `source: "fixtures"`; malformed or missing native fixtures never fall back to live reads. CLI stand-ins use one JSON state file per tool (`exit_code`, `stderr`, `calls`); they never replace the process's own effective user in a live invocation.

[GLOSSARY.md](GLOSSARY.md) defines the domain language. [docs/adr](docs/adr) records the platform, source, read-only and scope decisions.

The fleet cuts releases on an Apple Silicon Mac with Rosetta and stable Rust's `aarch64-apple-darwin` and `x86_64-apple-darwin` targets. The release sequence lives in [the fleet's cut configuration](docs/agents/afkconvoy.md); `packaging/release.sh` lists the individual operations. The formula is generated from the archives, with their URLs and SHA-256 hashes, and pushed to `sunfmin/homebrew-tap`.
