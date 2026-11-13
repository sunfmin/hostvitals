#!/usr/bin/env bash
# Tests ./verify.sh's choice of checker: Homebrew's sunfmin/tap/flux-toolchain,
# the newest the tap has, and nothing an older install left in ~/.cargo/bin or
# ~/.local/bin. A stand-in brew answers as Homebrew does for a formula that is
# not installed, installed and outdated, or installed and the newest; a
# stand-in cargo runs the `cargo-flux` it finds first on the PATH.
set -euo pipefail
cd "$(dirname "$0")"
root=$PWD
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
failures=0
fail() {
  echo "verify-test: $1" >&2
  failures=$((failures + 1))
}

# A copy of the checkout's scripts and manifests, so the run touches no
# target directory of the real one.
mkdir -p "$work/tree"
cp verify.sh Cargo.toml "$work/tree/"
cp -R crates "$work/tree/crates"
rm -rf "$work/tree/crates"/*/src

# brew: its state is the file $work/state (absent, outdated or newest); each
# call is logged to $work/brew.log.
mkdir -p "$work/bin" "$work/prefix/bin" "$work/home/.cargo/bin" "$work/home/.local/bin"
cat >"$work/bin/brew" <<EOF
#!/bin/sh
echo "\$*" >>"$work/brew.log"
state=\$(cat "$work/state")
case "\$1" in
update) exit 0 ;;
list) [ "\$state" = absent ] || echo "sunfmin/tap/flux-toolchain 2026.11.12" ;;
outdated) [ "\$state" != outdated ] || echo "sunfmin/tap/flux-toolchain" ;;
--prefix) echo "$work/prefix" ;;
install) echo newest >"$work/state" ;;
upgrade) echo newest >"$work/state" ;;
esac
EOF
# cargo: `cargo flux ...` runs the first cargo-flux on the PATH.
cat >"$work/home/.cargo/bin/cargo" <<'EOF'
#!/bin/sh
sub=$1
shift
exec "cargo-$sub" "$@"
EOF
for place in prefix/bin:tap home/.cargo/bin:cargo home/.local/bin:local; do
  printf '#!/bin/sh\necho "flux from %s"\n' "${place#*:}" >"$work/${place%%:*}/cargo-flux"
done
chmod +x "$work/bin/brew" "$work/home/.cargo/bin/cargo" "$work"/*/bin/cargo-flux "$work"/home/.*/bin/cargo-flux

# run <state> <verify.sh arguments>: prints what verify.sh printed, both
# streams, and its exit status on the last line.
run() {
  echo "$1" >"$work/state"
  : >"$work/brew.log"
  shift
  local status=0
  HOME="$work/home" PATH="$work/home/.local/bin:$work/home/.cargo/bin:$work/bin:/usr/bin:/bin" \
    "$work/tree/verify.sh" "$@" 2>&1 || status=$?
  echo "exit $status"
}

out=$(run absent --toolchain)
case "$out" in *"exit 1") ;; *) fail "--toolchain passes with no flux-toolchain installed: $out" ;; esac
case "$out" in *"brew install sunfmin/tap/flux-toolchain"*) ;; *) fail "not installed, and the fix is not named: $out" ;; esac

out=$(run outdated --toolchain)
case "$out" in *"exit 1") ;; *) fail "--toolchain passes with an outdated flux-toolchain: $out" ;; esac
case "$out" in *"brew upgrade sunfmin/tap/flux-toolchain"*) ;; *) fail "outdated, and the fix is not named: $out" ;; esac
grep -q "^update" "$work/brew.log" || fail "--toolchain compares with the tap unread"

out=$(run newest --toolchain)
[ "$out" = "exit 0" ] || fail "--toolchain fails with the newest flux-toolchain: $out"

out=$(run absent install)
[ "$out" = "exit 0" ] || fail "install leaves the toolchain wanting: $out"
grep -q '^install .*sunfmin/tap/flux-toolchain' "$work/brew.log" || fail "install does not brew install it: $(cat "$work/brew.log")"

out=$(run outdated install)
[ "$out" = "exit 0" ] || fail "install leaves an outdated toolchain wanting: $out"
grep -q '^upgrade .*sunfmin/tap/flux-toolchain' "$work/brew.log" || fail "install does not brew upgrade it: $(cat "$work/brew.log")"

out=$(run newest install)
grep -q -E '^(install|upgrade)' "$work/brew.log" && fail "install reinstalls the newest: $(cat "$work/brew.log")"

out=$(run outdated)
case "$out" in *"exit 1") ;; *) fail "a check runs with an outdated flux-toolchain: $out" ;; esac
case "$out" in *"flux from"*) fail "a check runs with an outdated flux-toolchain: $out" ;; esac

out=$(run newest)
case "$out" in *"exit 0") ;; *) fail "a check with the newest flux-toolchain fails: $out" ;; esac
case "$out" in *"flux from cargo"* | *"flux from local"*) fail "a check runs an older cargo-flux ahead of the tap's: $out" ;; esac
case "$out" in *"flux from tap"*) ;; *) fail "a check does not run the tap's cargo-flux: $out" ;; esac

# Nothing pins, builds or downloads the checker: the tap is its one source.
if grep -n -E '^\[workspace\.metadata\.checker\]|^(fixpoint|z3|nightly) = ' "$root/Cargo.toml"; then
  fail "Cargo.toml still pins a checker"
fi
if grep -n -E 'cargo xtask install|git clone https://github.com/flux-rs|liquid-fixpoint|Z3Prover|fixpoint --version|z3 --version' "$root/verify.sh"; then
  fail "verify.sh still pins, builds or downloads a checker"
fi

[ "$failures" -eq 0 ] || exit 1
echo "verify-test: ok"
