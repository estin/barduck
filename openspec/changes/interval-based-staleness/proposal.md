## Why

`stale_after` is a single global, fixed-duration threshold, but every other schedule knob (`interval`, `retry_interval`, `timeout`) is per-source. One global window can't fit sources with different cadences: a source polling every 5s won't flag stale until several missed cycles, while a source on a 6h `interval` or a daily `cron` sits in the `stale` (yellow) state for hours after every successful run purely because the fixed window is shorter than its own cadence. The only working knob today is hand-tuning `stale_after` to match the fastest source, which breaks the moment sources have mixed schedules. Deriving staleness from each source's own schedule instead removes the mismatch and the tunable both.

## What Changes

- **BREAKING**: Remove the global `Config.stale_after` field entirely — no top-level `stale_after` TOML key, no `default_stale()`, no `BARDUCK_STALE_AFTER` environment override.
- New staleness rule, derived per source from its own schedule:
  - Interval-scheduled source: stale once no successful fetch has landed within `2 × effective_interval()` — i.e. it has missed its second expected call.
  - Cron-scheduled source: stale immediately whenever it has not yet completed a successful run (no time-based re-check afterward — see design.md for why).
- Remove `stale_after` from the humantime duration-fields requirement and from the environment-variable override list.
- Update `demo/config.toml` and `tests/integration.rs` to drop the now-nonexistent `stale_after` key.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `data-collection`: "Health status derived from fetch outcomes" — staleness is now derived from each source's own interval/cron schedule instead of a single configured global window.
- `source-configuration`: "Environment variables override top-level settings" (drop `stale_after`/`BARDUCK_STALE_AFTER`) and "Human-readable duration configuration" (drop `stale_after` from the humantime field list).

## Impact

- `src/health.rs`: `compute()` looks up the source's own `SourceCfg` (interval or cron) instead of reading `cfg.stale_after`.
- `src/config/mod.rs`: remove the `stale_after` field and its `Default` wiring.
- `src/config/defaults.rs`: remove `default_stale()` and the `BARDUCK_STALE_AFTER` override branch.
- `tests/integration.rs`, `demo/config.toml`: drop the `stale_after` key.
- `openspec/specs/data-collection/spec.md`, `openspec/specs/source-configuration/spec.md`: updated per Capabilities above.
