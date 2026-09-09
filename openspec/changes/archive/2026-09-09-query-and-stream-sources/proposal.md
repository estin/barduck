## Why

Barduck fetches over both HTTP and shell, but every real deployment shells out anyway; the HTTP path (URL + dotted selector + `reqwest`) is dead weight that complicates the source model. Making shell the only transport — one-shot `Query` plus long-lived `Stream` with structured `jsonl` rows — lets commands push values, timestamps, and threshold updates through a single explicit schema.

## What Changes

- **BREAKING** Remove the `http` source type and all selector logic: `type = "http"` fails startup as an unknown type, `url`/`selector` fields are removed, and the `reqwest` client path is deleted.
- **BREAKING** Rename `type = "script"` to `type = "query"`: only `query` is accepted; `script` fails startup as an unknown type. A `Query` runs its shell command oneshot per tick on `interval` or `cron`, exactly as `script` did.
- Add `type = "stream"`: a long-running shell command whose stdout is a stream of `jsonl` values (one JSON object per line, each line a new reading). Streams take no `interval`/`cron`; they take only `expected_interval` — if no value arrives within it, the source reports stale. When the stream process ends, the collector reopens it after `retry_interval`.
- `Query` accepts `jsonl` output with backward compatibility: plain stdout is stored as the value exactly as today; if the output parses as a `jsonl` row it is applied structurally (see below).
- `jsonl` row schema: `{ value (required, string), ts (optional), threshold (optional, Vec<Threshold>) }`. `ts` becomes the reading's timestamp (invalid/missing falls back to arrival time); `threshold`, when present, persistently replaces the source's thresholds for that and all later readings. Unknown fields are rejected.
- Model `SourceCfg` as an explicit enum for serde deserialization so each source type carries only its own fields (query: `command` + schedule; stream: `command` + `expected_interval`), with unknown-type and mistyped-field configs rejected at startup naming the source and field.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities
- `source-configuration`: source type model becomes shell-only (`query`, `stream`); `http`/selector removed; `script` renamed; per-type fields and validation via an explicit `SourceCfg` enum; `jsonl` row schema and threshold-override persistence.
- `data-collection`: collection semantics for `query` (oneshot per tick, unchanged scheduling) and `stream` (continuous ingest, `expected_interval` staleness, reopen after `retry_interval` on exit); per-row `ts` timestamping.
- `data-storage`: reading provenance timestamp is the row's `ts` when a `jsonl` row carries a valid one, else collection arrival time.

## Impact

- Affected code: `src/config/source.rs` (`SourceType`, `SourceCfg` enum, validation), `src/source.rs` (drop `fetch_http`/selector/`HTTP_CLIENT`, query oneshot, stream line reader), `src/collector.rs` (per-type loops: interval/cron tick vs. persistent stream task + reconnect), `src/db.rs` (per-row timestamps, threshold persistence mechanism), health/staleness derivation for streams.
- **BREAKING** for existing configs using `type = "http"` or `type = "script"`, and for anything referencing `url`/`selector`.
- Dependencies: `reqwest` HTTP path removed; no new transport dependencies.
