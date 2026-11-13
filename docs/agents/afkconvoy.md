# afkconvoy config

How the afkconvoy fleet works this repo's backlog: issues labelled
`ready-for-agent`, worked by the team
[sunfmin/product-team](https://github.com/sunfmin/product-team), landed on
`main` behind `./gate.sh`, the same gate a person runs.

```yaml
team: sunfmin/product-team
base_branch: "main"
gate_command: afkconvoy exclusive -- ./gate.sh
# Flux runs alone before the tests; the limit leaves room for a cold cargo
# build and a cold Flux run.
gate_seconds: 3600
ready_label: ready-for-agent
```

The gate checks the policy crate with Flux, and `./verify.sh` checks it with
Homebrew's `sunfmin/tap/flux-toolchain`, and refuses to while it is not
installed or is older than the newest the tap has, the tap read fresh for the
check (sunfmin/control#851, afkconvoy's ADR-0067). The tap builds it every
day, with the `cargo-flux`, `fixpoint` and `z3` it runs, so nothing here pins
or builds any of them. So a host without it is refused at the startup probe,
and its work goes to a host that has it. The check is `verify.sh`'s own, run
without checking anything, and how to install it is what the check says when
it fails: `./verify.sh install`, which is `brew install
sunfmin/tap/flux-toolchain`, or `brew upgrade` of it where it is installed and
older, and does nothing where it is the newest. It is the need's `install`, so
the host runs it where the check fails, then probes again. Its limit covers a
bottle's download, with room for a slow network: nothing is built. The Rust
nightly the tap's Flux runs on is added by its first run, the gate's.

```yaml gate-needs
checks:
  - name: flux-toolchain
    command: ./verify.sh --toolchain
    seconds: 60
    install: ./verify.sh install
    install_seconds: 1800           # a download: nothing is built
```

The fleet cuts its first version, declared by `initial` below, when it goes
idle without a version tag. After that it cuts a version with landed work
past the newest `v*` tag (control ADR 0003). It lands the version change
through the train, tags that commit, publishes both Mac archives and the
generated formula, then upgrades the host's install. These are the release
steps, the only place their sequence is written. Each block runs under `sh -e`, with the
version in `$VERSION`: `set` on `main`, `publish` at the new tag.

```yaml cut
initial: 0.1.0
set: |
  set -ex
  packaging/release.sh set "$VERSION"
publish: |
  set -ex
  packaging/release.sh doctor
  packaging/release.sh check "$(git describe --tags --exact-match)"
  afkconvoy exclusive -- packaging/release.sh build
  packaging/release.sh package
  packaging/release.sh publish
  brew update --quiet
  unset HOMEBREW_NO_INSTALL_UPGRADE
  brew install --no-ask sunfmin/tap/hostvitals
  test "$("$(brew --prefix)/bin/hostvitals" --version)" = "hostvitals $VERSION"
```

`doctor` requires stable Rust's two Mac targets and a Mac that runs both
architectures (Apple Silicon with Rosetta). It names missing prerequisites
without installing them. Packaging checks each binary's architecture and
`--version` before making either archive. Publishing uploads the archives,
checksums and generated formula to this public repository's `v<version>`
release, then pushes `Formula/hostvitals.rb` to `sunfmin/homebrew-tap`.
The host's `gh` and `git` must be allowed to write both repositories.
Retries replace release assets and leave an identical tap formula alone.
Homebrew's `install` also upgrades an outdated installation, so the first
cut and later cuts use the same command without prompting.
