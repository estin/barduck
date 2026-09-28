# Design

## Context

See `proposal.md` (Why) for motivation. Current state (`src/main.rs`):
`Cli::config` is a `PathBuf` with clap `default_value = "config.toml"`,
so the default is fixed at parse time. XDG resolution, env override, and
existence-checked fallback all need the process environment and the
filesystem at runtime, which a static default string cannot express.
Everything downstream (`canonicalize` → `config::load`) already works on
an absolute or relative path, so only the default computation changes.

## Goals / Non-Goals

**Goals:**

- Resolution order: explicit `--config`/`-c` → `BARDUCK_CONFIG` (non-empty)
  → XDG default if the file exists → `./config.toml` if it exists →
  startup error naming the paths tried.
- The used path is printed to stderr on every successful startup.
- Existing `./config.toml` launch-directory setups keep working
  (non-breaking via the CWD fallback).

**Non-Goals:**

- No config-file search path beyond the two defaults, no migration that
  copies `./config.toml` into the XDG location.
- No change to `config::load`, schema, other env overrides, or validation.

## Decisions

- **Flag becomes `Option<PathBuf>` + `resolve_config_path()` helper**
  (`src/main.rs` or `config`): `Some(p)` → use as-is (still `canonicalize`d,
  so a missing explicit path errors as today). `None` → apply the
  env/XDG/CWD order with `Path::exists` checks. Rationale: clap cannot
  evaluate env/filesystem-dependent defaults; `Option` keeps
  `--config`/`-c`/help intact with minimal churn. Alternative (clap
  `default_value_t` with a `lazy_static`) rejected — env/filesystem lookup
  at static-init time breaks test injectability.
- **Env name `BARDUCK_CONFIG`** (non-empty, else ignored). Rationale:
  follows the project's `BARDUCK_*` convention (`BARDUCK_LISTEN`,
  `BARDUCK_HISTORY_POINTS`, …). It is intentionally *outside*
  `apply_env_overrides_from` (which handles in-file scalar overrides):
  this selects the file itself, so it must resolve before `config::load`
  runs. Alternative (`BARDUCK_CONFIG_PATH`) considered — rejected as
  longer with no precedent in the codebase.
- **`std::env` only, no `dirs`/`directories` crate**: read
  `XDG_CONFIG_HOME` (non-empty), else `HOME` + `/.config`, then append
  `barduck/config.toml`. Rationale: two env reads; a dependency for this
  is needless abstraction. Alternative (add `directories` crate) rejected
  per boring-design rule.
- **Empty `XDG_CONFIG_HOME` treated as unset** (falls back to
  `$HOME/.config`). Rationale: matches the XDG spec's own rule.
- **Missing `HOME` when needed → startup error naming the problem**,
  not a panic. Rationale: project denies panics on user-facing paths;
  a container without `HOME` should get "cannot resolve default config"
  rather than `expect`. The CWD fallback is still attempted first when
  reachable — order: explicit → env → XDG (if resolvable) → CWD → error.
- **Print the used path to stderr** (`eprintln!`), not stdout. Rationale:
  several subcommands emit `--json`/tables on stdout for scripting
  (`cli_report`); a notice on stdout would corrupt them. Stderr keeps it
  visible in terminals and out of pipes.
- **Existence checks via `Path::exists`, keep `canonicalize` after**:
  the helper returns the winning candidate; the existing `canonicalize`
  then normalizes it and still produces the "resolving config path …"
  error for races (deleted between check and load).

## Risks / Trade-offs

- [Risk] Two candidate files exist (XDG + CWD) — XDG silently wins, which
  may surprise a user who expected their `./config.toml` → Mitigation:
  the stderr print makes the winner visible on every run; spec scenarios
  pin the order.
- [Risk] Tests that relied on the CWD default now resolve elsewhere when
  run with a populated XDG home → Mitigation: pass explicit `--config`/
  temp paths in tests (already the norm in `tests/`); hermetic tests
  should set `XDG_CONFIG_HOME` to a temp dir.
- [Risk] `BARDUCK_CONFIG` pointing at a missing file errors (no further
  fallback) → Mitigation: intentional — an explicit operator choice must
  fail loudly, same as an explicit `--config`; scenario pinned in spec.

## Migration Plan

No migration needed: existing `./config.toml` setups without an XDG file
resolve exactly as before (plus a stderr notice). Users wanting the XDG
location move the file; rollback is revert. Note `database_path`/script
commands resolve against the config file's own directory, so a *relative*
`database_path` moves with the file — flag this in the release note, not
in code.
