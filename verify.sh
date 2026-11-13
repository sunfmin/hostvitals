#!/bin/sh
# Checks the refinement types of every crate that enables Flux (its
# Cargo.toml has `[package.metadata.flux]`), one crate at a time, so the last
# summary Flux prints in each run is that crate's.
# Needs Homebrew's `sunfmin/tap/flux-toolchain`, the newest the tap has: its
# `cargo-flux` brings the `fixpoint`, `z3` and Flux sysroot it was built with,
# and nothing here pins or builds any of them (sunfmin/control#851,
# afkconvoy's docs/adr/0067).
#
#   ./verify.sh                 check every Flux crate whole
#   ./verify.sh first           check the functions whose path contains a name
#   ./verify.sh <cargo flux arguments>
#   ./verify.sh --list          print the name of each Flux crate, and exit
#   ./verify.sh --toolchain     check only that the toolchain is installed and
#                               the newest the tap has, and exit: the gate need
#                               that docs/agents/afkconvoy.md declares
#   ./verify.sh install         install the toolchain, or upgrade it to the
#                               newest the tap has, then check as --toolchain
#                               does: that gate need's install
#
# Before each crate it prints `verify: checking <its directory>` on stdout.
set -e
cd "$(dirname "$0")"
if [ "${1:-}" = "--list" ]; then
    for manifest in crates/*/Cargo.toml; do
        if grep -q '^\[package\.metadata\.flux\]' "$manifest"; then
            sed -n 's/^name = "\(.*\)"$/\1/p' "$manifest"
        fi
    done
    exit 0
fi
formula=sunfmin/tap/flux-toolchain
# brew where the PATH has it, or in either place it installs to.
brew=$(command -v brew 2>/dev/null) || brew=
[ -n "$brew" ] || for b in "$HOME/homebrew/bin/brew" /opt/homebrew/bin/brew; do
    [ -x "$b" ] && brew=$b && break
done
if [ -z "$brew" ]; then
    echo "verify: Flux comes from Homebrew's $formula, and no brew is on the PATH, in ~/homebrew or in /opt/homebrew: install Homebrew, then: brew install $formula" >&2
    exit 1
fi
installed() { "$brew" list --versions "$formula" 2>/dev/null | grep -q .; }
outdated() { "$brew" outdated --quiet "$formula" 2>/dev/null | grep -q .; }
# The tap is read fresh before it is compared with: the newest is the newest
# the tap has now. A host that cannot reach it compares with what it last read.
read_tap() {
    "$brew" update --quiet >/dev/null 2>&1 ||
        echo "verify: brew update failed: $formula is compared with the tap as this host last read it" >&2
}
case "${1:-}" in
install)
    read_tap
    if ! installed; then
        "$brew" install --quiet "$formula" >&2
    elif outdated; then
        "$brew" upgrade --quiet "$formula" >&2
    fi
    set -- --toolchain
    ;;
--toolchain) read_tap ;;
esac
if ! installed; then
    echo "verify: $formula is not installed. Install it, with ./verify.sh install, or: brew install $formula" >&2
    exit 1
fi
if outdated; then
    echo "verify: $formula $("$brew" list --versions "$formula" | cut -d' ' -f2-) is not the newest the tap has. Upgrade it, with ./verify.sh install, or: brew upgrade $formula" >&2
    exit 1
fi
if [ "${1:-}" = --toolchain ]; then
    exit 0
fi
# The tap's cargo-flux comes first, so no cargo-flux, fixpoint or z3 an older
# install left in ~/.local/bin or ~/.cargo/bin is the one run: it puts its own
# fixpoint and z3 first. cargo-flux re-invokes `cargo +<its nightly>`, which
# only rustup's cargo understands; rustup starts from rust-toolchain.toml's.
PATH="$("$brew" --prefix "$formula")/bin:$HOME/.cargo/bin:$PATH"
export PATH
# Every run, the gate's or a hand's, stops a solver query of one function past
# twenty seconds as an error naming it, checks on past it so one run names
# every slow function, and times each, the file of timings kept out of the
# tree: a run is never held up by one query.
FLUXFLAGS="-Ftimings -Ffixpoint-timeout=20 -Fcatch-bugs -Flog-dir=$PWD/target/flux-log${FLUXFLAGS:+ $FLUXFLAGS}"
export FLUXFLAGS
case "${1:-}" in
"" | -* | check) ;;
*) set -- check --only-check="def:$1" ;;
esac
crates=$(LC_ALL=C grep -l '^\[package\.metadata\.flux\]' crates/*/Cargo.toml)
if [ -z "$crates" ]; then
    echo "verify: no crate enables Flux: a pure crate's Cargo.toml has [package.metadata.flux] with enabled = true" >&2
    exit 1
fi
for manifest in $crates; do
    echo "verify: checking ${manifest%/Cargo.toml}"
    cargo flux "$@" --manifest-path "$manifest"
done
