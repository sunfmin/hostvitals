#!/usr/bin/env bash
# Exercises release.sh at its command-line interface, with real Mach-O
# binaries and a local tap. GitHub is the only publishing stand-in.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$PWD
scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT

fail() { echo "release test: $*" >&2; exit 1; }
fixture() {
    repo="$scratch/repo"
    mkdir -p "$repo/packaging"
    cp packaging/release.sh "$repo/packaging/"
    cp Cargo.toml Cargo.lock "$repo/"
    cp -R crates "$repo/"
    printf '/target/\n/dist/\n' >"$repo/.gitignore"
    git -C "$repo" init -q
    git -C "$repo" config user.name 'Release Test'
    git -C "$repo" config user.email 'release-test@example.invalid'
    git -C "$repo" add .
    git -C "$repo" commit -qm 'Fixture'
    v=$("$repo/packaging/release.sh" version)
    git -C "$repo" tag "v$v"
}
run() { "$repo/packaging/release.sh" "$@"; }
refuses() {
    if run "$@" >"$scratch/output" 2>&1; then fail "accepted $*"; fi
}
binary() {
    destination=$1
    architecture=$2
    release=$3
    mkdir -p "$repo/target/$destination/release"
    printf '#include <stdio.h>\nint main(void) { puts("hostvitals %s"); return 0; }\n' "$release" >"$scratch/main.c"
    clang -arch "$architecture" "$scratch/main.c" -o "$repo/target/$destination/release/hostvitals"
}
binaries() {
    binary aarch64-apple-darwin arm64 "$v"
    binary x86_64-apple-darwin x86_64 "$v"
}

case "${1:-all}" in
all)
    for test in set check build package formula publish; do
        "$root/packaging/release-test.sh" "$test"
    done
    ;;
publish)
    fixture
    binaries
    run package >/dev/null
    export RELEASE_TEST_TAP="$scratch/tap.git" RELEASE_TEST_RELEASES="$scratch/releases"
    git init --bare -q "$RELEASE_TEST_TAP"
    git clone -q "$RELEASE_TEST_TAP" "$scratch/tap-seed" 2>/dev/null
    git -C "$scratch/tap-seed" -c user.name=Test -c user.email=test@example.invalid commit --allow-empty -qm 'Tap'
    git -C "$scratch/tap-seed" push -q origin HEAD
    mkdir -p "$scratch/tools" "$RELEASE_TEST_RELEASES"
    cat >"$scratch/tools/gh" <<'SH'
#!/bin/sh
set -eu
if [ "$1 $2" = 'repo clone' ]; then
    [ "${RELEASE_TEST_FAIL_TAP:-}" != yes ] || exit 1
    [ "$3" = sunfmin/homebrew-tap ] || exit 2
    exec git clone --quiet "$RELEASE_TEST_TAP" "$4"
fi
[ "$1" = release ] || exit 2
operation=$2
tag=$3
shift 3
destination=$RELEASE_TEST_RELEASES/$tag
case "$operation" in
view) [ -d "$destination" ]; exit ;;
create) mkdir -p "$destination" ;;
upload) [ -d "$destination" ] || exit 1 ;;
*) exit 2 ;;
esac
while [ "$#" -gt 0 ]; do
    case "$1" in
    -R|--repo) [ "$2" = sunfmin/hostvitals ] || exit 2; shift 2 ;;
    --title) shift 2 ;;
    --verify-tag|--generate-notes|--clobber) shift ;;
    *) cp "$1" "$destination/"; shift ;;
    esac
done
SH
    chmod +x "$scratch/tools/gh"
    export PATH="$scratch/tools:$PATH"
    export GIT_AUTHOR_NAME=Test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=Test GIT_COMMITTER_EMAIL=test@example.invalid
    export RELEASE_TEST_FAIL_TAP=yes
    refuses publish
    for file in "hostvitals-$v-macos-arm64.tar.gz" "hostvitals-$v-macos-x86_64.tar.gz" SHA256SUMS hostvitals.rb; do
        [ -s "$RELEASE_TEST_RELEASES/v$v/$file" ] || fail "release lacks $file before tap push"
    done
    unset RELEASE_TEST_FAIL_TAP
    run publish
    run formula >"$scratch/expected.rb"
    git --git-dir="$RELEASE_TEST_TAP" show HEAD:Formula/hostvitals.rb >"$scratch/pushed.rb"
    cmp "$scratch/expected.rb" "$scratch/pushed.rb"
    before=$(git --git-dir="$RELEASE_TEST_TAP" rev-parse HEAD)
    run publish
    [ "$(git --git-dir="$RELEASE_TEST_TAP" rev-parse HEAD)" = "$before" ] || fail 'retry made a redundant tap commit'
    cmp "$scratch/expected.rb" "$RELEASE_TEST_RELEASES/v$v/hostvitals.rb"
    (cd "$RELEASE_TEST_RELEASES/v$v" && shasum -a 256 -c SHA256SUMS)
    echo 'release test: publish passed'
    ;;
build)
    fixture
    mkdir -p "$scratch/tools"
    export RELEASE_TEST_CARGO
    RELEASE_TEST_CARGO=$(rustup which --toolchain stable cargo)
    export RELEASE_TEST_TOOLS="$scratch/tools" RELEASE_TEST_CALLS="$scratch/calls" RELEASE_TEST_VERSION="$v"
    cat >"$scratch/tools/rustup" <<'SH'
#!/bin/sh
case "$1" in
which) echo "$RELEASE_TEST_TOOLS/cargo" ;;
target) printf 'aarch64-apple-darwin\n'; [ "${RELEASE_TEST_NO_INTEL:-}" = yes ] || echo x86_64-apple-darwin ;;
*) exit 2 ;;
esac
SH
    cat >"$scratch/tools/cargo" <<'SH'
#!/bin/sh
set -eu
if [ "$1" != build ]; then exec "$RELEASE_TEST_CARGO" "$@"; fi
[ "$RUSTUP_TOOLCHAIN" = stable ] || exit 1
printf '%s\n' "$*" >>"$RELEASE_TEST_CALLS"
while [ "$1" != --target ]; do shift; done
target=$2
case "$target" in aarch64-apple-darwin) architecture=arm64 ;; x86_64-apple-darwin) architecture=x86_64 ;; *) exit 2 ;; esac
mkdir -p "target/$target/release"
printf '#include <stdio.h>\nint main(void) { puts("hostvitals %s"); return 0; }\n' "$RELEASE_TEST_VERSION" |
    clang -arch "$architecture" -x c - -o "target/$target/release/hostvitals"
SH
    chmod +x "$scratch/tools/"*
    export PATH="$scratch/tools:$PATH"
    export RELEASE_TEST_NO_INTEL=yes
    refuses doctor
    grep -q 'rustup target add --toolchain stable x86_64-apple-darwin' "$scratch/output"
    refuses build
    [ ! -e "$scratch/calls" ] || fail 'built before checking prerequisites'
    unset RELEASE_TEST_NO_INTEL
    run doctor
    run build
    [ "$(wc -l <"$scratch/calls" | tr -d ' ')" = 2 ] || fail 'did not build exactly two targets'
    for target in aarch64-apple-darwin x86_64-apple-darwin; do
        grep -q "build --release --locked -p hostvitals --target $target" "$scratch/calls"
    done
    run package >/dev/null
    echo 'release test: build passed'
    ;;
formula)
    fixture
    refuses formula
    binaries
    run package >/dev/null
    run formula >"$scratch/hostvitals.rb"
    ruby - "$scratch/hostvitals.rb" "$repo/dist" "$v" <<'RUBY'
require 'digest'
class Formula
  class << self
    attr_reader :urls, :hashes, :release_test
    def method_missing(*)
      nil
    end
    def on_arm(&block)
      block.call
    end
    alias on_intel on_arm
    def url(value)
      (@urls ||= []) << value
    end
    def sha256(value)
      (@hashes ||= []) << value
    end
    def test(&block)
      @release_test = block
    end
  end
  def version
    ENV.fetch('EXPECTED_VERSION')
  end
  def bin
    '/installed/bin'
  end
  def shell_output(command)
    raise command unless command == '/installed/bin/hostvitals --version'
    "hostvitals #{version}\n"
  end
  def assert_equal(expected, actual)
    raise "#{expected} != #{actual}" unless expected == actual
  end
end
load ARGV[0]
version = ARGV[2]
ENV['EXPECTED_VERSION'] = version
%w[arm64 x86_64].each_with_index do |arch, index|
  archive = "hostvitals-#{version}-macos-#{arch}.tar.gz"
  expected = "https://github.com/sunfmin/hostvitals/releases/download/v#{version}/#{archive}"
  raise Hostvitals.urls.inspect unless Hostvitals.urls[index] == expected
  raise 'wrong hash' unless Hostvitals.hashes[index] == Digest::SHA256.file("#{ARGV[1]}/#{archive}").hexdigest
end
formula = Hostvitals.new
raise 'root caveat' unless formula.caveats.include?('root') && formula.caveats.include?('skipped')
formula.instance_eval(&Hostvitals.release_test)
RUBY
    echo 'release test: formula passed'
    ;;
package)
    fixture
    binaries
    binary x86_64-apple-darwin x86_64 "$v+1.g1234567"
    refuses package
    [ ! -e "$repo/dist/hostvitals-$v-macos-arm64.tar.gz" ] || fail 'partial package before checking both versions'
    binary x86_64-apple-darwin arm64 "$v"
    refuses package
    binary x86_64-apple-darwin x86_64 "$v"
    run package
    for arch in arm64 x86_64; do
        file="$repo/dist/hostvitals-$v-macos-$arch.tar.gz"
        [ "$(tar -tzf "$file")" = hostvitals ] || fail "archive contents: $file"
        mkdir -p "$scratch/$arch"
        tar -xzf "$file" -C "$scratch/$arch"
        [ "$(lipo -archs "$scratch/$arch/hostvitals")" = "$arch" ] || fail "archive arch: $arch"
        [ "$(arch "-$arch" "$scratch/$arch/hostvitals" --version)" = "hostvitals $v" ] || fail "archive version: $arch"
    done
    (cd "$repo/dist" && shasum -a 256 -c SHA256SUMS)
    DIST="$scratch/archives with spaces" run package >/dev/null
    (cd "$scratch/archives with spaces" && shasum -a 256 -c SHA256SUMS)
    echo 'release test: package passed'
    ;;
check)
    fixture
    refuses check v9.9.9
    run check "v$v"
    echo '# changed' >>"$repo/Cargo.toml"
    refuses check "v$v"
    git -C "$repo" add Cargo.toml
    git -C "$repo" commit -qm 'Past the tag'
    refuses check "v$v"
    echo 'release test: check passed'
    ;;
set)
    fixture
    run set 7.8.9
    [ "$(run version)" = 7.8.9 ] || fail 'version was not set'
    for name in hostvitals hostvitals-policy; do
        got=$(awk -v name="$name" '$0 == "name = \"" name "\"" { found=1; next } found && /^version = / { print; exit }' "$repo/Cargo.lock")
        [ "$got" = 'version = "7.8.9"' ] || fail "$name lock version: $got"
    done
    cp "$repo/Cargo.lock" "$scratch/lock"
    run set 7.8.9
    cmp "$scratch/lock" "$repo/Cargo.lock"
    refuses set '1.2.3" bad'
    refuses set $'1.2.3\nbad'
    [ "$(run version)" = 7.8.9 ] || fail 'invalid version changed manifest'
    echo 'release test: set passed'
    ;;
*) fail 'usage: release-test.sh set|check|build|package|formula|publish' ;;
esac
