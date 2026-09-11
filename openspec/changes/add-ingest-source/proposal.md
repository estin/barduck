## Why

Barduck's `stream` source type requires a long-running shell command whose stdout is parsed as `jsonl`. For systems that push data via HTTP (webhooks, event buses, external services), running a command is unnecessary overhead — data arrives as a push, not a pull. An `ingest` source type lets users declare a named endpoint that receives HTTP push payloads, with staleness detection based on the silence window instead of a command's lifetime.

## What Changes

- New `ingest` variant on `SourceType` and `SourceCfg`: a source that
  has **no `command`** and receives data exclusively via HTTP push to
  the existing `/api/ingest` endpoint (added by the
  `add-http-ingest-endpoint` change).
- The `ingest` source declares `expected_interval` (humantime) for
  staleness: if no push arrives within that window the source reports
  `stale`, exactly as `stream` sources do.
- **BREAKING**: `ingest` is a new `type` value; configs using it must
  be aware that `SourceCfg` gains a third variant. The `query` and
  `stream` types are unchanged.
- `SourceKind` gains an `Ingest` variant (no command string).
- The collector skips `ingest` sources during scheduled collection
  (they have no interval/cron); their data arrives via push only.
- Health staleness detection (`is_stale`) handles `ingest` like
  `stream`: stale when `last_ok_age > expected_interval`.

## Capabilities

### New Capabilities

- `ingest-source`: the `ingest` source type declaration — config
  fields (`name`, `title`, `expected_interval`, `format`,
  `thresholds`, `show_in`, `value_type`), staleness semantics, and
  the fact that it has no command and receives data via HTTP push.

### Modified Capabilities

- `source-configuration`: adds `ingest` as a valid `type` value in the
  per-type source enum; `SourceCfg::Ingest` variant and `SourceType::Ingest`.
- `data-collection`: collector skips `ingest` sources on schedule
  ticks; `is_stale` in the health module handles `ingest` like `stream`.
- `http-api`: the `/api/ingest` endpoint now also accepts readings
  destined for `ingest`-type sources (it already accepts any
  config-declared source name).

## Impact

- `src/config/source.rs`: `SourceType` gains `Ingest`; `SourceCfg`
  gains `Ingest` variant; `SourceKind` gains `Ingest` variant; all
  accessor methods updated with a match arm.
- `src/source.rs`: `SourceKind::Ingest` variant; `build()` skips
  command check for ingest.
- `src/health.rs`: `is_stale` handles `ingest` like `stream`.
- `src/collector.rs`: `collect_once` skips `ingest` sources; new
  `Ingest` match arm in the collector dispatch.
- `src/config/validation.rs`: validates `ingest` sources have
  `expected_interval` and no `command`.
- Config files: `type = "ingest"` sources now accepted.
