## 1. Storage and origin

- [x] 1.1 Add `fetch_logs.origin` migration (`ADD COLUMN IF NOT EXISTS` + backfill `poll`), `COALESCE(origin,'poll')` in log SELECTs, `LogRow.origin`, and explicit-origin insert paths (existing callers pass `poll`); verify with a test that pre-migration rows read as `poll` and new rows keep their origin
- [x] 1.2 Verify the topcoat handler runtime can `await` `Db` ops and send on a tokio channel from `AppState` — done by code inspection: handlers are async fns driven by `topcoat::serve_until` inside the tokio runtime (`run_daemon`), and the existing `logs` handler already awaits `Db::logs_for_sources` (`spawn_blocking` inside); `mpsc::Sender::send().await` needs only the same runtime context, so no probe is required

## 2. Ingest pipeline

- [x] 2.1 Refactor `store_parsed_value` health refresh to re-derive status from the DB so the HTTP handler can call it without task-local `last_status`; verify existing collector and health tests still pass unmodified
- [x] 2.2 Implement `POST /api/ingest` (body validation, unknown source → 4xx, type conversion, session bands, `push`-origin log row, success echo); verify with handler tests for store, unknown-source, and bad-payload cases
- [x] 2.3 Add per-source reset senders to `AppState` and the reset arm in the collector loop (interval sources only; deadline becomes `now + interval`); verify with a test that ingest defers the next tick and clears retry backoff while cron/stream tasks are untouched

## 3. Presentation

- [x] 3.1 Render ORIGIN in the CLI `logs` table and `--json` output; verify with a CLI test over mixed-origin rows
- [x] 3.2 Render origin in the web per-source log view; verify by requesting the page for a source with both origins

## 4. Verification

- [x] 4.1 Run the full suite (`cargo test`) plus `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`, all green; verify end-to-end with a live daemon (ingest → reading visible, interval deferred, `logs --source` shows `push`)
