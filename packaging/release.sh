#!/bin/sh
# Release operations. The fleet's sequence lives in docs/agents/afkconvoy.md.
#
#   version          print the version Cargo builds
#   set <version>    set the workspace and lockfile versions (X.Y.Z)
#   doctor           check this Mac's stable Rust targets and ability to run both
#                    Mac architectures; report missing prerequisites with fixes
#   check <tag>      require v<version> at HEAD, with a clean checkout
#   build            build both macOS binaries on stable, at the clean tag
#   package          check both binaries' arch and --version, then archive them
#   formula          generate a Homebrew formula from the archives on stdout
#   release          upload archives, checksums and generated formula to GitHub
#   tap              generate and push Formula/hostvitals.rb to sunfmin/tap
#   publish          release, then tap; safe to retry after a partial failure
#
# DIST and CARGO_TARGET_DIR override dist/ and target/. Nothing is installed.
set -eu
cd "$(dirname "$0")/.."
ROOT=$PWD
SCRIPT=$ROOT/packaging/release.sh

NAME=hostvitals
REPO=sunfmin/hostvitals
TAP=sunfmin/homebrew-tap
ARCHES="arm64 x86_64"
DIST=${DIST:-dist}
TARGET_DIR=${CARGO_TARGET_DIR:-target}

# Pin both cargo and the rustc it launches to rustup's stable toolchain.
stable() {
    rustup=$(PATH="$PATH:$HOME/.cargo/bin" command -v rustup) || rustup=
    cargo=$([ -n "$rustup" ] && "$rustup" which --toolchain stable cargo 2>/dev/null) || cargo=
    if [ -z "$cargo" ]; then
        echo "release: rustup's stable toolchain is required: rustup toolchain install stable --profile minimal" >&2
        exit 1
    fi
    PATH="$(dirname "$cargo"):$PATH"
    RUSTUP_TOOLCHAIN=stable
    export PATH RUSTUP_TOOLCHAIN
}

version() {
    package_id=$(cargo pkgid --manifest-path "$ROOT/Cargo.toml" --offline --locked -p "$NAME") || return 1
    printf '%s\n' "$package_id" | sed 's/.*[#@]//'
}

check_tag() {
    want="v$(version)"
    at=$(git describe --tags --exact-match --match "$want" 2>/dev/null) || at=
    if [ "$1" != "$want" ] || [ "$at" != "$want" ] || [ -n "$(git status --porcelain)" ]; then
        echo "release: requires a clean checkout of $want, with tag $want: commit changes and check out that tag" >&2
        exit 1
    fi
}

target() {
    case "$1" in
    arm64) echo aarch64-apple-darwin ;;
    x86_64) echo x86_64-apple-darwin ;;
    esac
}

archive() {
    echo "$NAME-$(version)-macos-$1.tar.gz"
}

# Both must pass before either archive is created. No version check is skipped.
check_binary() {
    built=$TARGET_DIR/$(target "$1")/release/$NAME
    if [ ! -f "$built" ]; then
        echo "release: $built is missing: packaging/release.sh build" >&2
        exit 1
    fi
    if [ "$(lipo -archs "$built")" != "$1" ]; then
        echo "release: $built is not a thin $1 binary: packaging/release.sh build" >&2
        exit 1
    fi
    said=$(arch "-$1" "$built" --version) || {
        echo "release: cannot check $built --version: packaging/release.sh doctor" >&2
        exit 1
    }
    if [ "$said" != "$NAME $(version)" ]; then
        echo "release: $built --version says '$said', expected '$NAME $(version)': packaging/release.sh build at the clean release tag" >&2
        exit 1
    fi
}

need_archives() {
    for architecture in $ARCHES; do
        if [ ! -f "$DIST/$(archive "$architecture")" ]; then
            echo "release: $DIST/$(archive "$architecture") is missing: packaging/release.sh package" >&2
            exit 1
        fi
    done
}

# Every release binary must be runnable here, including Intel under Rosetta.
doctor() {
    if [ "$(uname -s)" != Darwin ]; then
        echo "release: build on an Apple Silicon Mac with Rosetta; this host is $(uname -s)" >&2
        exit 1
    fi
    stable
    installed=$("$rustup" target list --installed --toolchain stable)
    lacks=
    for architecture in $ARCHES; do
        rust_target=$(target "$architecture")
        if ! printf '%s\n' "$installed" | grep -qx "$rust_target"; then
            echo "release: stable lacks $rust_target: rustup target add --toolchain stable $rust_target" >&2
            lacks=yes
        fi
        if ! arch "-$architecture" /usr/bin/true 2>/dev/null; then
            case "$architecture" in
            x86_64) echo 'release: Intel execution requires Rosetta: softwareupdate --install-rosetta --agree-to-license' >&2 ;;
            arm64) echo 'release: arm64 execution requires an Apple Silicon Mac: run the release there' >&2 ;;
            esac
            lacks=yes
        fi
    done
    [ -z "$lacks" ] || exit 1
}

case "${1:-}" in
version)
    stable
    version
    ;;
set)
    to=${2:?usage: release.sh set <version>}
    case "$to" in
    ''|*[!0-9.]*)
        echo "release: version must be X.Y.Z: packaging/release.sh set 1.2.3" >&2
        exit 2
        ;;
    esac
    if ! printf '%s\n' "$to" | grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$'; then
        echo "release: '$to' is not X.Y.Z: packaging/release.sh set 1.2.3" >&2
        exit 2
    fi
    stable
    sed -i.bak "/^\[workspace\.package\]/,/^\[/ s/^version = \".*\"\$/version = \"$to\"/" Cargo.toml
    rm Cargo.toml.bak
    cargo update --offline --workspace
    [ "$(version)" = "$to" ] || {
        echo "release: Cargo.toml does not build $to: check [workspace.package].version" >&2
        exit 1
    }
    ;;
check)
    stable
    check_tag "${2:?usage: release.sh check <tag>}"
    ;;
doctor)
    doctor
    echo 'release: this Mac builds and runs both macOS architectures'
    ;;
build)
    doctor
    check_tag "v$(version)"
    for architecture in $ARCHES; do
        cargo build --release --locked -p "$NAME" --target "$(target "$architecture")"
    done
    ;;
package)
    stable
    check_tag "v$(version)"
    for architecture in $ARCHES; do check_binary "$architecture"; done
    mkdir -p "$DIST"
    for architecture in $ARCHES; do
        built=$TARGET_DIR/$(target "$architecture")/release/$NAME
        COPYFILE_DISABLE=1 tar -czf "$DIST/$(archive "$architecture")" --no-xattrs -C "$(dirname "$built")" "$NAME"
        echo "$DIST/$(archive "$architecture")"
    done
    (cd "$DIST" && for architecture in $ARCHES; do shasum -a 256 "$(archive "$architecture")"; done) >"$DIST/SHA256SUMS"
    ;;
formula)
    stable
    need_archives
    base=https://github.com/$REPO/releases/download/v$(version)
    cat <<FORMULA
class Hostvitals < Formula
  desc "Read-only observations of a Mac and its processes for agents"
  homepage "https://github.com/$REPO"
  version "$(version)"
  license "MIT"

  depends_on :macos

  on_arm do
    url "$base/$(archive arm64)"
    sha256 "$(shasum -a 256 "$DIST/$(archive arm64)" | cut -d' ' -f1)"
  end
  on_intel do
    url "$base/$(archive x86_64)"
    sha256 "$(shasum -a 256 "$DIST/$(archive x86_64)" | cut -d' ' -f1)"
  end

  def install
    bin.install "hostvitals"
  end

  def caveats
    <<~EOS
      Some measurements need root and are skipped without it.
      hostvitals never runs sudo itself; skipped measurements name the command
      you can choose to run. Start with \`hostvitals --help\`, then
      \`hostvitals doctor\` to check this Mac's observation capabilities.
    EOS
  end

  test do
    assert_equal "hostvitals #{version}", shell_output("#{bin}/hostvitals --version").strip
  end
end
FORMULA
    ;;
release)
    stable
    check_tag "v$(version)"
    need_archives
    formula=$DIST/$NAME.rb
    "$SCRIPT" formula >"$formula"
    ruby -c "$formula" >/dev/null
    (cd "$DIST" && for architecture in $ARCHES; do shasum -a 256 "$(archive "$architecture")"; done) >"$DIST/SHA256SUMS"
    set -- "$DIST/SHA256SUMS" "$formula"
    for architecture in $ARCHES; do set -- "$@" "$DIST/$(archive "$architecture")"; done
    if gh release view "v$(version)" -R "$REPO" >/dev/null 2>&1; then
        gh release upload "v$(version)" -R "$REPO" --clobber "$@"
    else
        gh release create "v$(version)" -R "$REPO" --verify-tag --title "$NAME $(version)" --generate-notes "$@"
    fi
    ;;
tap)
    stable
    check_tag "v$(version)"
    need_archives
    formula=$DIST/$NAME.rb
    "$SCRIPT" formula >"$formula"
    ruby -c "$formula" >/dev/null
    tap=$(mktemp -d)
    trap 'rm -rf "$tap"' EXIT
    gh repo clone "$TAP" "$tap" -- --quiet --depth 1
    mkdir -p "$tap/Formula"
    cp "$formula" "$tap/Formula/$NAME.rb"
    git -C "$tap" add "Formula/$NAME.rb"
    if git -C "$tap" diff --cached --quiet; then
        echo "release: the tap already has $NAME $(version)"
    else
        git -C "$tap" commit --quiet -m "$NAME $(version)"
        git -C "$tap" push --quiet origin HEAD
    fi
    ;;
publish)
    "$SCRIPT" release
    "$SCRIPT" tap
    ;;
*)
    awk 'NR > 1 && !/^#/ { exit } NR > 1' "$SCRIPT" >&2
    exit 2
    ;;
esac
