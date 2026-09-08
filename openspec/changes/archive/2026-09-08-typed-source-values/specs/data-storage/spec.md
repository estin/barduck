## ADDED Requirements

### Requirement: Typed value columns
The `readings` table SHALL include nullable `value_bigint` (BIGINT), `value_double` (DOUBLE), and `value_json` (JSON) columns in addition to the existing `value` (VARCHAR) column. When a source declares a `value_type` (spec: source-configuration — Configurable stored value type) other than the default `string`, each of its readings SHALL have the matching typed column populated with that reading's typed representation, so it can be queried and used in SQL math/analytics directly, without re-parsing the string column. The `value` column SHALL still be populated for every reading regardless of `value_type`, so existing consumers of the string column are unaffected. No migration path is provided for a database file created before this change; such a file MUST be recreated before use with the new schema.

#### Scenario: Typed column populated alongside the string column
- **WHEN** a source with `value_type = "double"` records a reading
- **THEN** the row's `value` column holds the string form and its `value_double` column holds the parsed double, with `value_bigint` and `value_json` left `NULL`

#### Scenario: Default-typed source leaves new columns null
- **WHEN** a source with no `value_type` (or `value_type = "string"`) records a reading
- **THEN** the row's `value` column is populated as before and `value_bigint`, `value_double`, and `value_json` are all `NULL`
