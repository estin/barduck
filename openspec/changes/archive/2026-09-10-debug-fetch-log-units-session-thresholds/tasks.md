## 1. Log view units

- [x] 1.1 Render each log entry's value together with the source's unit (bare value when no unit), matching the panel form, and verify with a web test asserting a banded USD source's log row shows value plus unit

## 2. Debug fetch command

- [x] 2.1 Add the `fetch` CLI subcommand (source name arg, `--json` flag) routing to a dry-run that writes nothing to the database, and verify `--help` lists it and unknown sources fail naming the source
- [x] 2.2 Implement query dry-run (source-timeout-bounded run, full parse pipeline: value, resolved ts, thresholds, value-type conversion) with human and JSON output, and verify with tests for plain output, a `jsonl` row, and a hanging command hitting the timeout
- [x] 2.3 Implement stream dry-run (spawn, print first parsed lines up to 5 within the source timeout, kill the command) and verify with a test using a finite multi-line command plus DB-untouched assertions (no readings/logs/health rows)

## 3. Session-only thresholds

- [x] 3.1 Replace the DB-backed override path with a daemon-shared in-memory band map (empty at startup, written by ingest, read by health/renderers with config-band fallback), stop calling `set_thresholds` from ingest, and verify overrides apply live and vanish across a simulated restart (fresh map reseeds from config)
- [x] 3.2 Keep direct-mode renderers on config bands and leave the `source_thresholds` table inert but readable, and verify existing old-DB tolerance tests still pass

## 4. Verification

- [x] 4.1 Run the full suite (`cargo test`), `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`, and verify all pass
