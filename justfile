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


demo:
    just run daemon --config demo/config.toml

# Update Cargo.lock, holding back any dependency version published more
# recently than cooldown.toml's window allows (see
# https://crates.io/crates/cargo-cooldown) so a compromised just-published
# release isn't pulled in before the ecosystem has had a chance to notice.
update-deps:
    cargo cooldown update
