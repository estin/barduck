## Context

See proposal.md for motivation. Current state: `SourceCfg` (`src/config/source.rs`) is a flat struct with `kind: SourceType` (`http`/`script`), optional `url`/`selector`/`command`, and `interval`/`cron`/`retry_interval` validated by hand-written checks; `source::build` maps it to `SourceKind::Http{url, selector}` (fetched via a shared `reqwest` client with dotted-path selector) or `SourceKind::Script{command}` (oneshot `run_shell`). The collector (`src/collector.rs`) schedules interval/cron ticks per source with last-attempt startup resume; `Db::insert_reading`/`insert_log` stamp arrival time via `now()`; health derives staleness from `last_success` age against schedule-derived windows.

## Goals / Non-Goals

**Goals:**
- Shell-only source model: `query` oneshot per tick, `stream` continuous ingest, per-type fields enforced by deserialization.
- Structured `jsonl` rows with `value`/`ts`/`threshold` semantics per the specs.

**Non-Goals:**
- No rate limiting or backpressure policy for chatty streams (ingest as fast as lines arrive).
- No DB schema migration: readings/fetch-log tables unchanged; row `ts` and threshold overrides ride existing columns plus collector-local state.
- No config auto-migration: `http`/`script` fail loudly with unknown-type errors.

## Decisions

- **Model `SourceCfg` as a `#[serde(tag = "type")]` enum (`Query{…}` / `Stream{…}`) with `deny_unknown_fields` per variant.** Per-type field sets and unknown-type rejection then fall out of deserialization (errors name source + field via the existing config-load context) instead of hand-written membership checks. Alternative (keep the flat struct, extend validation) rejected: it re-implements what serde already proves, and the user explicitly asked for the enum.
- **Effective thresholds live in per-source collector state, seeded from config.** A row `threshold` replaces the in-memory bands used by renderers/health for that and later readings; a daemon restart reseeds from config (the command re-emits its bands anyway). Alternative (a DB table for overrides) rejected: schema migration and cross-restart merge rules for what is effectively a display hint the producer owns.
- **Stream ingest is one task per source: spawn → line-split stdout → per-line row handling → on exit, `insert_log` + sleep `retry_interval` → reopen.** Malformed lines become failed fetch-log entries without killing the stream; shutdown stops reopening after the current wait, mirroring the existing graceful tasks, and the child is killed via the existing process-group guard. Alternative (batch lines per tick) rejected: it would blur per-row timestamps and threshold sequencing.
- **`Query` output detection: trimmed stdout parsed as a JSON object containing a string `value` key → structural row; anything else → plain value.** Only objects qualify (arrays/numbers are plain values, preserving backward compatibility for numeric/string outputs). Unknown row fields fail the row per the schema rather than being ignored.
- **Row `ts` accepts RFC 3339 or epoch seconds (int/float); invalid or absent → arrival time, reading still recorded.** Fetch-log entries always keep arrival timestamps (so last-attempt resume math is never skewed by producer clocks); only the reading carries the row instant. `value_type` conversion applies to the extracted row `value` exactly like a plain value.
- **Delete the HTTP path wholesale: `fetch_http`, `HTTP_CLIENT`, dotted selector, `url`/`selector` fields, and the `reqwest` dependency.** No fallback transport remains, so there is nothing to feature-gate.

## Risks / Trade-offs

- [Chatty streams flood the DB with readings] → Mitigation: none in this change (documented non-goal); `expected_interval` governs staleness display, not ingest rate. A future sampling policy can layer on top.
- [Threshold override lost on restart until the command re-emits] → Mitigation: accepted and spec'd; commands emitting bands on early lines converge within seconds of startup.
- [Producer-clock `ts` in the future skews history display] → Mitigation: stored as-is for provenance (fetch-log resume math unaffected since it uses arrival time); UI orders by stored timestamp as today.
- [Long-lived children outliving shutdown] → Mitigation: reuse the existing process-group kill guard; collector tasks already stop after their current wait.

## Migration Plan

- Breaking config change with no data migration: users rename `type = "script"` → `type = "query"`, replace `http` sources with shell equivalents, and give streams `expected_interval`. Rollback is a plain revert; old configs keep working on the old binary since the DB schema is untouched.

## Open Questions

- None.
