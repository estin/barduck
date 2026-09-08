## Context

`health::compute()` (`src/health.rs`) currently computes staleness as `now - last_success_ts > cfg.stale_after`, a single `Duration` on the top-level `Config` (default `30m`, overridable via `BARDUCK_STALE_AFTER`). `compute()` already receives `cfg: &Config` and the source name, so it can already look up that source's `SourceCfg` (via `cfg.sources.iter().find(...)`) the same way `source_visible_in` does. `SourceCfg::effective_interval()` and the `cron` field already exist and are mutually exclusive (source-configuration spec: "Cron schedule"). See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Derive staleness per source from its own schedule, with no remaining global tunable.
- Keep `compute()`'s existing shape (bounded queries, `SourceHealth` output) — this is a threshold-source change, not a rework of health derivation.

**Non-Goals:**
- Modeling cron cadence to compute an expected next-fire time (e.g. via `croner`) for time-based cron staleness. Cron schedules are irregular by nature (daily, weekly, "3rd of month"), so a generic "expected gap" isn't derivable from the expression alone without picking an arbitrary lookback window. Out of scope here.
- Changing `failure_threshold`/failing-status derivation, or the retry-interval behavior from the prior `source-fetch-retry-interval` change.

## Decisions

**Staleness window = `2 × effective_interval()` for interval sources.** "Missed its second expected call" is the plain reading of the request. It's computed at read time in `compute()` from `SourceCfg::effective_interval()` — no new config field, no new stored state. A source with no recorded success has `last_ok_age = f64::INFINITY`, which already exceeds any finite window, so "never run" falls out of the same comparison without a separate branch.

**Cron sources: stale only until their first success, never re-evaluated by age afterward.** Alternative considered: parse the cron expression and flag stale after, say, `2×` the shortest gap between the last few occurrences. Rejected — same reasoning as the Non-Goals entry above: a cron expression doesn't carry a single "expected interval," so any derived number would be a guess dressed up as a rule. The chosen behavior matches today's request literally ("if source wasn't run by cron - mark it as stale immediately") and is a strict behavior narrowing versus today's global-window logic, which did apply *some* age check to cron sources (using the 5m default-interval fallback via `effective_interval()`, an already-meaningless number for a cron source). No source-visible regression: a cron source that has proven it works is trusted between runs, the same way a `setup`-gated source is trusted once its setup succeeds.

**No per-source override field.** An alternative would keep a `stale_after`-like knob but move it to `SourceCfg` as an optional override. Rejected: the request removes the global knob outright rather than relocating it, so there is intentionally no staleness config left anywhere; behavior is fully derived from `interval`/`cron`.

**`compute()` looks up `SourceCfg` internally rather than changing its signature.** It already takes `cfg: &Config`; adding `cfg.sources.iter().find(|s| s.name == source)` inside is a smaller diff than threading a `&SourceCfg` through every call site, and matches the existing pattern in `source_visible_in`.

## Risks / Trade-offs

- **A cron source whose schedule silently stops firing (e.g. the cron expression itself is fine but something upstream broke) shows healthy forever once it has succeeded once** → Mitigated only by `failure_threshold`/failing status if it's actually being attempted and erroring; if it stops being *attempted* at all there's no signal. Accepted per the Non-Goals decision above; flagged here so it isn't rediscovered as a surprise later.
- **Very short intervals (e.g. `interval = "5s"`) now flag stale after 10s of no success**, tighter than the old fixed 30m default → this is the intended fix (the whole point is that the window should track the source's own cadence), but is a behavior change worth calling out in tasks.md's manual-check step.
- **Removing `stale_after` is a breaking config change** → any existing config or `.env` setting `stale_after` / `BARDUCK_STALE_AFTER` now fails startup (unrecognized field / unused env var, per "Human-readable duration configuration"'s existing reject-unknown-fields behavior). No migration shim — call this out plainly in the proposal (already does) and in release notes at merge time.
