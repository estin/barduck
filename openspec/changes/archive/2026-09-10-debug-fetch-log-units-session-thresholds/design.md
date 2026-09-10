## Context

See proposal.md for motivation. Current state (after `query-and-stream-sources`): `jsonl` threshold overrides are written to the `source_thresholds` DB table via the writer task and read back by `Db::effective_thresholds`, which feeds `SourceHealth.thresholds` for every renderer. Overrides therefore survive restarts. The log view (`log_rows` in `src/web/routes.rs`) renders bare values; panels render `value + unit`. There is no dry-run command: `collect_once` always writes.

## Goals / Non-Goals

**Goals:**
- Overrides live exactly as long as the daemon process; restart = config bands.
- One dry-run path reusing the real parse pipeline, with zero DB writes.
- Log value cells match the panel `value + unit` form.

**Non-Goals:**
- No migration to delete `source_thresholds` rows or the table (left inert, ignored).
- No streaming/line-count flags on the debug command beyond the fixed first-5-lines behavior.

## Decisions

- **Hold overrides in a daemon-shared `RwLock<HashMap<String, Vec<Threshold>>>` owned by `AppState`, seeded empty at startup.** Collector stream/query ingest writes it; `health::compute` (which already takes `&Config`) prefers it over declared bands when present; `SourceHealth.thresholds` keeps carrying the effective bands so no renderer signatures change. Alternative (keep the DB table, clear on startup) rejected: a crash or second direct-mode reader would resurrect stale overrides, and "forgotten on stop" becomes "forgotten on clean start" — weaker than spec'd. Alternative (per-source task-local state) rejected: renderers couldn't see it without new plumbing anyway.
- **Direct-mode (no daemon) renderers show config bands.** Without a session there is nothing to override from; `health::compute` falls back to declared bands when no session map is supplied (parameterize with `Option<&SessionBands>` or resolve before the call at the AppState boundary). TUI/`collect_once`/tests are unaffected.
- **`Db::set_thresholds` stops being called by ingest; `effective_thresholds` stays as a fallback reader.** Keeps the read path working against databases that still carry old rows (ignored only in the sense that nothing new is written and session state wins when present — decide: session map wins over DB rows; DB rows are legacy). Hmm, simpler alternative considered: delete the table code outright. Rejected: `reset` and old-DB tolerance already depend on it; leaving the reader costs nothing and keeps `effective_thresholds` useful for direct mode.
- **Debug command reuses `source::parse_output` + `convert_value_type`, then prints and exits.** Query: `timeout()`-bounded `run_shell`. Stream: `StreamProc::spawn`, up to 5 `next_line()` calls each bounded by the remaining source timeout, then `shutdown()`. Output struct `{ source, value, ts, threshold, value_type_result }` rendered as a small table (human) or JSON (`--json`, via the existing `--json` flag pattern). Follows the existing `QueryArgs`-style flag plumbing in `main.rs`.
- **Log unit rendering reuses the panel form.** The log view already resolves `src`; append `unit` exactly like panels do (`"{value} {unit}"`, bare value when no unit). Threshold coloring logic untouched.

## Risks / Trade-offs

- [Session map lost on crash like everything else in memory] → Mitigation: that IS the spec (forgotten on stop, however it stops).
- [Direct-mode TUI never shows overrides] → Mitigation: accepted; overrides are daemon-session state by definition.
- [`source_thresholds` table lingers with stale rows] → Mitigation: documented vestigial; `reset` still clears it.

## Migration Plan

- No data migration. Deploy: overrides stop persisting from first new-daemon start; stale rows ignored (session map wins). Rollback: plain revert; old binary keeps reading the table.

## Open Questions

- None.
