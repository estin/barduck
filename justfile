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
