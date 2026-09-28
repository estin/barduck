# Proposal

## Why

The daemon's config currently defaults to `./config.toml`, tying every invocation to the launch directory. A per-user dashboard binary should follow the XDG Base Directory convention so one config (`barduck/config.toml` under the user's config home) works from anywhere, while existing launch-directory setups keep working.

## What Changes

- The default config resolution becomes: explicit `--config`/`-c` wins; else `BARDUCK_CONFIG` env wins; else `$XDG_CONFIG_HOME/barduck/config.toml` (`$HOME/.config` when `XDG_CONFIG_HOME` is unset or empty) when that file exists; else `./config.toml` when it exists; else startup fails with an error naming the paths tried.
- The resolved config path is printed to stderr on startup (stdout stays clean for `--json` consumers).
- Help text and user-facing docs (`SKILL.md`, `README.md` where they state the default) updated to the new resolution.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `source-configuration`: "Config file defines sources" gains the default location rule — flag/env/XDG/CWD precedence, XDG base-directory resolution, CWD fallback, the used-config startup notice, and the failure case when nothing exists.

## Impact

- Affected code: `src/main.rs` (`Cli::config` — a `clap` static `default_value` cannot express an env-dependent path, so the flag becomes `Option<PathBuf>` with a runtime-computed default; plus resolution helper and stderr print).
- No new dependencies (`std::env` only); no config-file schema change; `config::load`/validation untouched.
- Assumes standard XDG resolution (`XDG_CONFIG_HOME`, else `~/.config`) and env name `BARDUCK_CONFIG` (follows the `BARDUCK_*` convention) — recorded here because clarification was declined; revisit if either assumption is wrong.
- Tests invoking the binary without `--config` and docs stating the old default need updates.
