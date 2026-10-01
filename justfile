default:
    @just --list

# The full gate: formatting, lints, then the fast test run. `--locked`
# throughout, because the repo carries a `cooldown.toml` supply-chain policy
# (see `update-deps`) that a silent re-resolve would quietly undo.
ci:
    cargo fmt --all -- --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo nextest run --locked

# The same suite built with `--all-features`, kept as its own slow recipe:
# that feature set turns on `bundled`, which compiles DuckDB from source
# instead of downloading a prebuilt library, so it costs far more than the
# default (unbundled) set above. CI runs it in a separate job; locally it is
# the one to run before a release rather than on every change.
ci-all-features:
    cargo nextest run --locked --all-features

# Build the release binary and its asset bundle together, then run it. They
# must come from the same build: an asset's bundle id includes the build
# directory it was declared in, so a bundle from a different build (a stale
# `cargo install`, a previous `--features` set) doesn't match the ids the
# binary renders. Pass args through, e.g.
# `just run daemon -c demo/config.toml`.
run *ARGS:
    cargo build --release
    topcoat asset bundle --release
    ./target/release/barduck {{ARGS}}

# Release smoke test: build the shippable artifact (release binary +
# matching asset bundle), boot the daemon on the demo config, and prove
# the dashboard renders. The bundle must come from the same build as the
# binary (see `run`); `BUNDLE_TARGET_DIR` selects the cross target dir
# for musl release builds, defaulting to the native one.
release-check:
    #!/usr/bin/env bash
    set -euo pipefail
    TARGET_DIR="${BUNDLE_TARGET_DIR:-target}"
    cargo build --locked --release --features bundled
    topcoat asset bundle --release
    BIN="$PWD/$TARGET_DIR/release/barduck"
    test -x "$BIN"
    # Fully static when built for musl; native glibc builds stay dynamic.
    if ldd "$BIN" 2>&1 | grep -q "statically linked"; then
        echo "static binary confirmed"
    elif [[ "$TARGET_DIR" != "target" ]]; then
        echo "expected a statically linked binary in $TARGET_DIR" >&2
        exit 1
    fi
    TMPDIR="$(mktemp -d)"
    trap 'kill $DAEMON_PID 2>/dev/null; rm -rf "$TMPDIR"' EXIT
    cp demo/config.toml "$TMPDIR/smoke.toml"
    (cd "$TMPDIR" && "$BIN" daemon -c smoke.toml >daemon.log 2>&1) &
    DAEMON_PID=$!
    for _ in $(seq 1 60); do
        curl -sf http://127.0.0.1:18420/ -o "$TMPDIR/page.html" && break
        sleep 1
    done
    grep -q 'id="bd-panel-wrapper"' "$TMPDIR/page.html"
    grep -q 'bd-width-toggle' "$TMPDIR/page.html"
    echo "release smoke test passed"

demo:
    just run daemon --config demo/config.toml

# Cut a release: bump the package version, regenerate CHANGELOG.md with
# git-cliff (see cliff.toml), commit both, tag `v<version>`, and push so
# the CI `release` job builds and publishes the static musl artifact.
# Usage: `just release` picks the next version from the conventional
# commits since the last tag (`git cliff --bumped-version`); `just release
# 0.2.0` (or `v0.2.0`) sets it explicitly. Refuses to run with a dirty
# tree, an existing tag, or a version older than the current one.
release VERSION="":
    #!/usr/bin/env bash
    set -euo pipefail
    test -z "$(git status --porcelain)" || { echo "dirty tree — commit or stash first" >&2; exit 1; }
    git fetch --tags --quiet origin
    VER="{{VERSION}}"
    [[ -n "$VER" ]] || VER="$(git cliff --bumped-version 2>/dev/null)"
    VER="${VER#v}"
    [[ "$VER" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "not a version: '$VER' (want X.Y.Z)" >&2; exit 1; }
    CUR="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "barduck") | .version')"
    # Equal is allowed: the current version may never have been tagged (the
    # tag check below is what stops re-releasing it).
    printf '%s\n%s\n' "$CUR" "$VER" | sort -VC || { echo "version $VER older than $CUR" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/v$VER" >/dev/null || { echo "tag v$VER already exists" >&2; exit 1; }
    test ! -e Cargo.toml.bak
    sed -i.bak -E "s/^(version = \").*(\")/\\1$VER\\2/" Cargo.toml
    rm Cargo.toml.bak
    # Cargo.lock records the package's own version too; refresh just that
    # entry (no dependency changes) so the `--locked` builds still pass.
    cargo update --workspace --offline --quiet
    cargo check --locked --offline >/dev/null 2>&1 || cargo check --locked >/dev/null
    git cliff --tag "v$VER" -o CHANGELOG.md
    git add Cargo.toml Cargo.lock CHANGELOG.md
    git commit -m "chore(release): v$VER"
    git tag "v$VER"
    git push origin HEAD "v$VER"

# Pin the flake's `barduck-bin` package (nix/release.json) to a published
# release's x86_64 tarball: download it and record its URL and SRI hash.
# The release workflow runs this after publishing; run it by hand to
# re-pin. `VERSION` defaults to Cargo.toml's (with or without the `v`).
# Needs only curl, openssl and jq — no Nix.
release-pin VERSION="":
    #!/usr/bin/env bash
    set -euo pipefail
    VER="{{VERSION}}"
    [[ -n "$VER" ]] || VER="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "barduck") | .version')"
    VER="${VER#v}"
    URL="https://github.com/estin/barduck/releases/download/v$VER/barduck-v$VER-linux-x86_64.tar.gz"
    TMP="$(mktemp)"
    trap 'rm -f "$TMP"' EXIT
    curl -fsSL --retry 3 -o "$TMP" "$URL"
    HASH="sha256-$(openssl dgst -sha256 -binary "$TMP" | openssl base64 -A)"
    jq -n --arg version "$VER" --arg url "$URL" --arg hash "$HASH" \
        '{version: $version, url: $url, hash: $hash}' > nix/release.json
    echo "pinned barduck-bin to v$VER ($HASH)"

# Update Cargo.lock, holding back any dependency version published more
# recently than cooldown.toml's window allows (see
# https://crates.io/crates/cargo-cooldown) so a compromised just-published
# release isn't pulled in before the ecosystem has had a chance to notice.
update-deps:
    cargo cooldown update
