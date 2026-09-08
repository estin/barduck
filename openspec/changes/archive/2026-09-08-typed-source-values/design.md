## Context

`readings.value` is `VARCHAR NOT NULL` (`src/db.rs:263-270`); every write goes through `Db::insert_reading`/`WriteCmd::InsertReading` (`src/db.rs:69-76, 508-537`), and the fetched value itself is always a plain `String` coming out of `SourceKind::fetch` (`src/source.rs:38-45`). Schema creation is `CREATE TABLE IF NOT EXISTS` (`src/db.rs:253-293`) with no migration or version table — `Db::reset` just drops and recreates. `SourceCfg` (`src/config/source.rs:132-185`) already has one rendering-only type-like field, `format: Option<ValueFormat>` (`src/config/layout.rs:8-31`), which is orthogonal to what this change adds: `format` controls how the web UI/TUI render a value; `value_type` controls what SQL type it's stored as. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Let a source declare a storage type (`bigint`, `double`, `json`) so its readings are queryable with native SQL types straight from the DuckDB file.
- Leave every existing consumer of `value` (web UI, TUI, HTTP API, threshold-band coloring) working unchanged.

**Non-Goals:**
- Surfacing the typed columns through the HTTP API, web UI, or TUI — this change only adds them to the raw table.
- Migrating or otherwise preserving database files created before this change — none is provided; such a file must be recreated.
- Retroactively converting historical rows written before a source declared a `value_type`.

## Decisions

- **New enum `ValueType` in `src/config/source.rs`** (`String` default, `Bigint`, `Double`, `Json`), a plain `#[serde(rename_all = "lowercase")]` derive on the `SourceCfg.value_type: Option<ValueType>` field — the same mechanism `SourceType`/`View`/`Level` already use, so an unknown value is rejected at deserialization with no separate `validation.rs` check needed. (`ValueFormat`'s `VALUE_FORMATS` + `parse(&str)` pattern exists only because `Cell::Text.format` lives inside an untagged enum, where a derive failure would silently fall through to a different variant instead of naming the bad value — that constraint doesn't apply to `SourceCfg`, a plain tagged struct.) Kept as a separate type from `ValueFormat` rather than reusing/extending it, since the two are independent axes (rendering vs. storage) and conflating them would force every existing `format` call site to reason about SQL types it doesn't care about.
- **Conversion happens in the collector, not in `db.rs`.** `src/collector.rs`'s `fetch_once` already owns the fetch → insert pipeline; it converts the fetched `String` against the source's `value_type` (via helpers in `src/source.rs`, e.g. `value.trim().parse::<i64>()`, `parse::<f64>()`, `serde_json::from_str`) before calling insert. A conversion failure is handled exactly like today's HTTP/script failures: log the fetch failure and return early, without touching the writer. This reuses the existing failure path instead of adding a second one inside the DB layer.
- **`insert_reading`/`WriteCmd::InsertReading` gain typed `Option` parameters** (`value_bigint: Option<i64>`, `value_double: Option<f64>`, `value_json: Option<String>`) rather than an enum, since the SQL statement needs one bind position per column regardless of type, and at most one will ever be `Some` for a given call.
- **Schema change is a direct edit to the `readings` `CREATE TABLE IF NOT EXISTS` statement** in `create_schema` — the three columns are added to the column list, same as any other column. No `ALTER TABLE`, version table, or migration runner: a database file created before this change simply won't have the columns and must be recreated, per the proposal's explicit no-migration decision.
- **`value_json` stored via `CAST(? AS JSON)`** rather than DuckDB's `json()` function, so an invalid JSON string surfaces as a bind/execute error the collector can treat uniformly with the `bigint`/`double` parse-failure path (both become "conversion failed → fetch failure"), instead of silently storing something unexpected.

## Risks / Trade-offs

- [A misconfigured `value_type` turns a previously-succeeding source into a persistently failing one] → This is intentional (matches the proposal's explicit behavior), but worth calling out: enabling `value_type` on an existing source is a config change that can change its health status. Mitigated by the fetch log naming the exact conversion error, same as any other fetch failure.
- [Opening a database file created before this change fails or misbehaves once the code expects the new columns] → Accepted per the proposal's explicit **BREAKING** call-out: no migration is provided. The fix is to delete/recreate the database file before running the new binary.
- [`value_bigint`/`value_double` precision] → Uses `i64`/`f64` (Rust) mapping directly to DuckDB `BIGINT`/`DOUBLE`; no arbitrary-precision numeric support is added, matching what the proposal asked for.
