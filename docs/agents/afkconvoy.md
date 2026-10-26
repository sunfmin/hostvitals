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
