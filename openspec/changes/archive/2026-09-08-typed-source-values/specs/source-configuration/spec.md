## ADDED Requirements

### Requirement: Configurable stored value type
A source MAY declare a `value_type` field controlling how its fetched value is stored: `string` (default, unchanged behavior), `bigint`, `double`, or `json`. Any other value MUST be rejected at startup, naming the source and the invalid value. When a source's `value_type` is not `string`, every fetched value MUST convert cleanly to that type (a valid integer for `bigint`, a valid number for `double`, valid JSON text for `json`); a value that fails to convert MUST be treated the same as a transport-level fetch failure (spec: source-configuration — Generic http source type, Generic script source type): no reading is recorded, and the attempt is recorded in the fetch log as failed, naming the conversion error.

#### Scenario: Default preserves current behavior
- **WHEN** a source declares no `value_type`
- **THEN** its readings are stored exactly as before this change, with no typed column populated

#### Scenario: Bigint value stored
- **WHEN** a source declares `value_type = "bigint"` and its fetch returns `"42"`
- **THEN** the reading is recorded with its typed bigint representation stored alongside the existing string value

#### Scenario: Double value stored
- **WHEN** a source declares `value_type = "double"` and its fetch returns `"98.6"`
- **THEN** the reading is recorded with its typed double representation stored alongside the existing string value

#### Scenario: JSON value stored
- **WHEN** a source declares `value_type = "json"` and its fetch returns `{"ok":true}`
- **THEN** the reading is recorded with its typed JSON representation stored alongside the existing string value

#### Scenario: Non-convertible value fails the fetch
- **WHEN** a source declares `value_type = "bigint"` and its fetch returns `"not-a-number"`
- **THEN** no reading is recorded, and the fetch log entry records the failure and the conversion error

#### Scenario: Invalid value_type rejected
- **WHEN** a source declares `value_type = "decimal"`
- **THEN** startup fails naming the source and the invalid value
