# Tasks

## 1. XDG default config path with fallback

- [x] 1.1 Change `Cli::config` to `Option<PathBuf>` with a `resolve_config_path()` helper implementing flag → `BARDUCK_CONFIG` → existing XDG default → existing `./config.toml` → error naming tried paths, and print the used path to stderr; verify `--help` shows the flag optional, an explicit `-c` still wins, and the notice goes to stderr (not stdout)
- [x] 1.2 Cover resolution with tests (XDG set hit, XDG unset/empty → `$HOME/.config`, missing XDG → CWD fallback, explicit flag wins over env+XDG, env wins over XDG, nothing exists errors naming paths, used path printed to stderr) and verify with `cargo test` for the affected tests

## 2. Callers and docs

- [x] 2.1 Update in-repo callers/docs stating the old default (`justfile`, `demo/README.md`, `SKILL.md`, `README.md` as applicable) to the new resolution and verify by grepping for stale `config.toml` default claims
- [x] 2.2 Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and the full test suite to verify no regressions
