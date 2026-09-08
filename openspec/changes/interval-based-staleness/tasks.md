## 1. Health derivation

- [x] 1.1 In `src/health.rs::compute()`, replace the `cfg.stale_after`-based comparison with a per-source rule: look up the source's `SourceCfg` from `cfg.sources`, and if it has a `cron` schedule, treat `last_success.is_none()` as stale (skip the age comparison entirely); otherwise compare `last_ok_age > 2.0 * source.effective_interval().as_secs_f64()`. Verify with new unit tests covering: interval source within its window (healthy), interval source past 2×interval with no success (stale), cron source with no success yet (stale), cron source with an old-but-present success (not stale on that basis).

## 2. Config removal

- [x] 2.1 Remove the `stale_after` field and its doc comment from `Config` in `src/config/mod.rs`, and its entry in `Config::default()`. Verify `cargo build` succeeds with no leftover references.
- [x] 2.2 Remove `default_stale()` from `src/config/defaults.rs` and the `BARDUCK_STALE_AFTER` branch in `apply_env_overrides_from()`. Verify `cargo build` succeeds and `grep -rn stale_after src/` finds nothing outside `health.rs`'s new per-source logic.
- [x] 2.3 Update the `unparseable_env_override_rejected` test in `src/config/mod.rs` (currently asserts on `BARDUCK_STALE_AFTER`) to exercise a different still-existing duration override (e.g. `BARDUCK_INTERVAL=not-a-duration`). Verify `cargo nextest run -p barduck config::` passes.

## 3. Fixtures and demo

- [x] 3.1 Remove the `stale_after = "30m"` line from `tests/integration.rs`'s `test_config()`. Verify `cargo nextest run` passes end to end.
- [x] 3.2 Remove the `stale_after = "1m"` line and its preceding comment from `demo/config.toml`; update `demo/README.md` if it documents the old global-window behavior. Verify `just demo` starts without a config error.

## 4. Spec sync

- [x] 4.1 Run `openspec validate --change interval-based-staleness --strict` and fix any reported issues.

## 5. Final checks

- [x] 5.1 Run `just ci` (clippy + nextest) and confirm it passes clean.
- [x] 5.2 Manually verify the new behavior with `demo/config.toml`: shorten one source's `interval` to a few seconds, stop the daemon before its next tick would land, and confirm the web UI/TUI shows it `stale` shortly after `2×interval` rather than after the old fixed window.
