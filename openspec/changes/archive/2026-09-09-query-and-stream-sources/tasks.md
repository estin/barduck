## 1. Config model

- [x] 1.1 Replace `SourceType`/`SourceCfg` with a `#[serde(tag = "type")]` enum (`query`, `stream`) carrying per-type fields and verify startup rejects `type = "http"`, `type = "script"`, and cross-type fields with errors naming the source and field (`cargo test config`)
- [x] 1.2 Add `expected_interval` (required humantime for `stream`, rejected with `interval`/`cron`) and keep `retry_interval` as the stream reopen delay, and verify with config validation tests covering missing, conflicting, and invalid durations
- [x] 1.3 Add the `jsonl` row struct (`value` required string, `ts` optional, `threshold` optional `Vec<Threshold>`, unknown fields rejected) with `ts` parsing (RFC 3339 or epoch seconds, fallback to now) and verify with unit tests for full, minimal, unknown-field, missing-value, and bad-ts rows

## 2. Source runtime

- [x] 2.1 Delete the HTTP path (`fetch_http`, shared client, selector logic) and drop the `reqwest` dependency, and verify `cargo build` shows no remaining references (`grep -rn "reqwest\|fetch_http\|selector" src/ ` returns only historical comments) and `cargo test source` passes
- [x] 2.2 Implement `Query` oneshot fetch (plain stdout backward compatible, JSON-object-with-`value` applied structurally, non-zero exit logged with status/stderr) and verify with source unit tests for plain, jsonl, and failing commands
- [x] 2.3 Implement the stream line ingestor (per-line row handling, malformed line → failed fetch-log entry without killing the stream, process exit → log + reopen after `retry_interval`, shutdown-aware with process-group cleanup) and verify with a test that emits valid, malformed, and valid lines then exits and is reopened

## 3. Collector and health

- [x] 3.1 Route `query` sources through the existing interval/cron schedule (unchanged freshness resume) and `stream` sources through the continuous ingest task with setup gating both paths, and verify `cargo test collector` passes plus setup-gates-stream coverage
- [x] 3.2 Apply row `ts` to readings (fetch logs keep arrival time), persist row `threshold` into per-source effective bands seeded from config, and derive stream staleness from `expected_interval`, and verify with health tests for silent-stream-stale and emit-recovers plus a reading-timestamp test
- [x] 3.3 Update all `http`/`script` references in CLI text, errors, docs, and remaining tests to `query`/`stream`, and verify with a full `grep` pass and the complete suite

## 4. Verification

- [x] 4.1 Run the full suite (`cargo test`), `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` on touched files, and verify everything passes
- [x] 4.2 Run an end-to-end daemon check with one `query` (plain + jsonl outputs) and one `stream` source (lines, bad line, exit/reopen, silence → stale) and verify readings, fetch logs, thresholds, and health match the specs
