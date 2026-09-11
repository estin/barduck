## 1. Config Types

- [x] 1.1 Add `Ingest` variant to `SourceType` enum in `src/config/source.rs` with `as_str()` returning `"ingest"`
- [x] 1.2 Add `Ingest` variant to `SourceCfg` tagged enum with fields: `name`, `title`, `expected_interval`, `format`, `thresholds`, `history_points`, `show_history`, `show_in`, `value_type` — no `command`, `interval`, `cron`, `retry_interval`
- [x] 1.3 Add `Ingest` variant to `SourceKind` enum in `src/source.rs` (zero-field)
- [x] 1.4 Add `Ingest` to `SourceCfg::kind()` and all accessor methods: `command()` returns `""`, `cron()` returns `None`, `interval()` returns `None`, `expected_interval()` returns `Some(expected_interval)`, `effective_interval()` returns `expected_interval`

## 2. Source Build & Validation

- [x] 2.1 Update `source::build()` to handle `SourceCfg::Ingest` — return `SourceKind::Ingest`, skip the command-presence check
- [x] 2.2 Add validation for `ingest` sources: require `expected_interval`, reject `command`, `interval`, `cron` fields at config validation time
- [x] 2.3 Update `SourceCfg::effective_retry_interval()` or add a method so `ingest` sources don't attempt retry logic (no process to reopen)

## 3. Collector Integration

- [x] 3.1 Update `spawn_graceful` to skip `ingest` sources — no collector task needed
- [x] 3.2 Update `collect_once` to skip `ingest` sources
- [x] 3.3 Add `Ingest` match arm in `loop_source` dispatch (e.g., just wait on shutdown, since ingest has no fetch loop)

## 4. Health & Staleness

- [x] 4.1 Update `is_stale` in `src/health.rs` to handle `ingest` sources like `stream` — stale when `last_ok_age > expected_interval`
- [x] 4.2 Add test: ingest source with no pushes reports stale after expected interval elapses

## 5. Tests

- [x] 5.1 Add config test: `ingest` source parses correctly with `expected_interval`
- [x] 5.2 Add config test: `ingest` source missing `expected_interval` fails validation
- [x] 5.3 Add config test: `ingest` source with `command` or `interval` fails validation
- [x] 5.4 Add health test: `ingest` source staleness follows `expected_interval` like `stream`
- [x] 5.5 Run `cargo test` and verify all tests pass

## 6. Demo & Documentation

- [x] 6.1 Add `ingest` source example to `demo/config.toml`
- [x] 6.2 Update `SKILL.md` with `ingest` source type documentation
