#!/usr/bin/env bash
# Tests how ./verify.sh --toolchain judges the installed checker, with stand-in
# `cargo`, `fixpoint` and `z3` that say what they are as the real ones do:
# cargo-flux and z3 must be the ones Cargo.toml pins, and fixpoint, which
# follows liquid-fixpoint's nightly release (sunfmin/control#732), any build
# that reports its version. The tools are put where verify.sh looks first,
# $HOME/.cargo/bin, under a HOME of the test's own, with only the system's
# directories after it on PATH.
set -euo pipefail
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0
bin="$work/home/.cargo/bin"
mkdir -p "$bin"

flux_rev=$(sed -n 's/^flux-core = .*rev = "\([0-9a-f]*\)".*/\1/p' Cargo.toml)
z3_pin=$(sed -n 's/^z3 = "\(.*\)"$/\1/p' Cargo.toml)

# tool <name> <first line of its --version, or nothing to leave it out>
tool() {
  rm -f "$bin/$1"
  [ -n "$2" ] || return 0
  printf '#!/bin/sh\necho "%s"\n' "$2" >"$bin/$1"
  chmod +x "$bin/$1"
}
# cargo-flux is run as `cargo flux --version`.
flux() { tool cargo "cargo-flux ${1:0:10} (2026-10-09)"; }
fixpoint() {
  if [ -n "$1" ]; then
    printf '#!/bin/sh\nprintf "\\nLiquid-Fixpoint Copyright 2009-25 Regents of the University of California.\\nAll Rights Reserved.\\n\\nfixpoint %s\\n"\n' "$1" >"$bin/fixpoint"
    chmod +x "$bin/fixpoint"
  else
    rm -f "$bin/fixpoint"
  fi
}
z3() { tool z3 "${1:+Z3 version $1 - 64 bit}"; }

# expect <name> <status> <stderr substring>
expect() {
  local name=$1 want=$2 says=$3 status=0
  HOME="$work/home" PATH="/usr/bin:/bin" ./verify.sh --toolchain 2>"$work/err" || status=$?
  if [ "$status" -ne "$want" ] || ! case $(cat "$work/err") in *"$says"*) true ;; *) false ;; esac; then
    echo "FAIL $name: exit $status, wanted $want; stderr: $(cat "$work/err"); wanted it to say: ${says:-anything}" >&2
    failures=$((failures + 1))
  else
    echo "ok   $name"
  fi
}

flux "$flux_rev"
z3 "$z3_pin"
fixpoint "0.9.6.3.8 (684d0829c19fbee83b892c9f5e20c2598897297c)"
expect "the formerly pinned fixpoint" 0 ""
# The 2026-11 nightly the host installed, which turned main red while
# fixpoint was pinned (sunfmin/control#855).
fixpoint "0.9.6.3.8 (0b924442ce26a8f270bffac18ba6295bc7411646)"
expect "a later nightly fixpoint" 0 ""
fixpoint "0.9.7.0 (69e585b4aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)"
expect "a newer fixpoint version" 0 ""

fixpoint ""
expect "no fixpoint" 1 "fixpoint reports no version"
fixpoint "0.9.6.3.8 (0b924442ce26a8f270bffac18ba6295bc7411646)"

z3 "4.13.0"
expect "another z3" 1 "z3 is 4.13.0, and Cargo.toml pins $z3_pin"
z3 ""
expect "no z3" 1 "z3 is (nothing reported)"
z3 "$z3_pin"

flux "0000000000000000000000000000000000000000"
expect "another cargo-flux" 1 "cargo-flux is built from Flux 0000000000"
tool cargo ""
expect "no cargo-flux" 1 "cargo-flux is built from Flux (nothing reported)"

[ "$failures" -eq 0 ] || exit 1
