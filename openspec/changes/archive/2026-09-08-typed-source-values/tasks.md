## 1. Schema

- [x] 1.1 Add `value_bigint BIGINT`, `value_double DOUBLE`, `value_json JSON` to the `readings` table's `CREATE TABLE IF NOT EXISTS` column list in `create_schema` (`src/db.rs`); verify with a test asserting the columns exist on a freshly created database (`DESCRIBE readings` or equivalent)
- [x] 1.2 Extend `Db::insert_reading` and `WriteCmd::InsertReading` to accept `value_bigint: Option<i64>`, `value_double: Option<f64>`, `value_json: Option<String>` and bind them into the `INSERT`, casting the JSON parameter with `CAST(? AS JSON)`; verify with a unit test that inserts one reading per type and reads back the row confirming only the matching typed column is non-null

## 2. Config

- [x] 2.1 Add `ValueType` enum (`String` default, `Bigint`, `Double`, `Json`) in `src/config/source.rs`, and a `value_type: Option<ValueType>` field on `SourceCfg`; verify with a unit test that TOML omitting `value_type` deserializes to `None`/default `String`
- [x] 2.2 Reject an unrecognized `value_type` string at startup, naming the invalid value; verify with a unit test asserting the error message names the bad value (implemented as a derived `#[serde(rename_all = "lowercase")]` enum — same mechanism as `SourceType`/`View`/`Level` — rather than a hand-written `validation.rs` check, since `SourceCfg` is a plain tagged struct with no untagged-enum ambiguity to work around; design.md's decision text updated to match)

## 3. Collector

- [x] 3.1 In `src/collector.rs`'s `fetch_once`, convert the fetched string against the source's `value_type` (`str::parse::<i64>`, `str::parse::<f64>`, or pass-through for `json`) before inserting, passing the converted value into the new `insert_reading` parameters; verify with a unit test per type confirming a valid value inserts with the matching typed column set
- [x] 3.2 On conversion failure, record a failed fetch-log entry with the conversion error and skip the insert, mirroring existing HTTP/script failure handling; verify with a unit test that an unconvertible value produces no reading row and a failed fetch-log entry naming the error

## 4. Integration & Regression

- [x] 4.1 Add an integration test in `tests/integration.rs` covering a `bigint`-typed and a `json`-typed source end-to-end (config → fetch → stored row), asserting the typed columns and unchanged `value` column
- [x] 4.2 Add an integration test confirming a source with no `value_type` produces rows identical in shape to today (all three new columns `NULL`), so existing behavior is provably unchanged
- [x] 4.3 Run `just ci` (clippy + nextest) and confirm it passes with no regressions
