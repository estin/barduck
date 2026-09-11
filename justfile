default:
    @just --list

ci:
    cargo clippy --all-targets -- -D warnings
    cargo nextest run

# Build the release binary and its asset bundle together, then run it (they
# must come from the same build — see docs/asset.md's OUT_DIR note — so this
# never uses `cargo install`). Pass args through, e.g.
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
