## Why

Today every reading is stored as `readings.value VARCHAR`, so any numeric or JSON analysis directly against the DuckDB file requires callers to `CAST`/parse text themselves, and that casting is error-prone (locale-sensitive numbers, malformed JSON) since the database has no idea what shape a source's values are supposed to have. Letting a source declare its value type lets the daemon store the value pre-validated and pre-typed, so raw SQL against the database file can do math and analytics directly on typed columns instead of re-parsing strings every time.

## What Changes

- Add an optional per-source `value_type` config field (`string` (default, unchanged behavior), `bigint`, `double`, or `json`) declaring what type a source's fetched value must be stored as.
- Add three new nullable columns to the `readings` table: `value_bigint BIGINT`, `value_double DOUBLE`, `value_json JSON`. The existing `value VARCHAR NOT NULL` column keeps holding the raw string form of every reading unchanged (so existing rendering, threshold-band parsing, and history bars keep working); when a source declares a non-`string` `value_type`, the matching typed column is additionally populated for that reading.
- A fetched value that fails to convert to its source's declared `value_type` (e.g. `value_type = "bigint"` but the source returns non-numeric text) is treated as a failed fetch: no reading is recorded, and the fetch log/health status reflect the failure, consistent with existing HTTP/script failure handling.
- **BREAKING**: no migration path is provided for existing database files. The `readings` table's `CREATE TABLE` definition simply gains the three new columns; a database file created before this change must be deleted (or recreated) before running the new code, since it won't have them. `value_type` itself is optional and defaults to `string`, which reproduces today's storage behavior for any source that doesn't opt in.

Out of scope: the web UI, TUI, and HTTP API continue to read/render the existing `value` string column exactly as today. This change only adds typed columns for direct SQL/analytics access to the raw DuckDB file; no UI or API surfaces the new columns.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `source-configuration`: adds the `value_type` field, its allowed values, startup validation of unknown values, and the fetch-failure behavior when a value doesn't match its declared type.
- `data-storage`: adds the typed `readings` columns, describes how they're populated per source, and requires in-place upgrade of existing database files.

## Impact

- `src/db.rs`: `create_schema` (add the three columns directly to the `readings` `CREATE TABLE`), `Db::insert_reading` / `WriteCmd::InsertReading` (accept and store the typed value alongside the string value).
- `src/config/source.rs`: new `ValueType` enum (`String` default, `Bigint`, `Double`, `Json`) and `SourceCfg.value_type` field; validation alongside existing `ValueFormat`/threshold validation.
- `src/collector.rs`: convert/validate the fetched string against the source's `value_type` before inserting; on conversion failure, record a failed fetch attempt exactly like existing HTTP/script failures instead of inserting a reading.
- No changes to `src/web/*`, `src/tui.rs`, `src/api.rs`, or `src/cli_report.rs` — they keep reading `value` as before.
