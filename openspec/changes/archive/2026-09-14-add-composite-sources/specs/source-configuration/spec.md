## ADDED Requirements

### Requirement: Composite source children

A `query` source MAY declare a non-empty `children` list, forming a **composite source**. Each child declares a bare name unique among its own siblings; the pair `<parent-name>::<child-name>` becomes that child's full name, addressable anywhere a source name is accepted, and MUST be unique across the whole source table exactly like any other source's `name`. A bare child name MUST NOT itself contain `::`. A child MAY declare `title`, `unit`, `format`, `thresholds`, `show_history`, `show_in`, `value_type`, and `history_points`; it MUST NOT declare `command`, `interval`, `cron`, `timeout`, `setup`, or `retry_interval` — any of those on a child MUST be rejected at startup naming the child's full name and the offending field. `children` MUST NOT be declared on a `stream` or `ingest` source, nor on a child itself (no nesting). A declared `children` list MUST NOT be empty.

#### Scenario: Valid composite parses into addressable sources

- **WHEN** a `query` source named `load` declares children `1m`, `5m`, and `15m`
- **THEN** `load::1m`, `load::5m`, and `load::15m` are each addressable as ordinary sources, alongside `load` itself

#### Scenario: Child declaring its own command is rejected

- **WHEN** a child entry declares a `command` field
- **THEN** startup fails naming the child's full name and the `command` field

#### Scenario: Child declaring its own schedule is rejected

- **WHEN** a child entry declares `interval`, `cron`, `timeout`, `setup`, or `retry_interval`
- **THEN** startup fails naming the child's full name and the offending field

#### Scenario: Duplicate full name is rejected

- **WHEN** a child's full name collides with another declared source's name (a top-level source or another composite's child)
- **THEN** startup fails naming the collision

#### Scenario: Child bare name containing the separator is rejected

- **WHEN** a child's bare `name` contains `::`
- **THEN** startup fails naming the child and the invalid name

#### Scenario: Children on a non-query source type is rejected

- **WHEN** a `stream` or `ingest` source declares `children`
- **THEN** startup fails naming the source

#### Scenario: Empty children list is rejected

- **WHEN** a `query` source declares `children = []`
- **THEN** startup fails naming the source

### Requirement: Composite root field restrictions

A composite source's own `unit`, `thresholds`, `value_type`, `format`, and `show_history` fields MUST be rejected at startup, naming the source and the offending field — none of them apply to a value the composite root itself never produces; its children carry those fields individually instead. `title`, `show_in`, `interval`/`cron`, `timeout`, `setup`, and `retry_interval` remain valid on the root exactly as for any other `query` source.

#### Scenario: Root declaring a per-value field is rejected

- **WHEN** a composite source declares `unit`, `thresholds`, `value_type`, `format`, or `show_history`
- **THEN** startup fails naming the source and the offending field

#### Scenario: Root's operational fields remain valid

- **WHEN** a composite source declares `title`, `show_in`, `interval`, `timeout`, `setup`, or `retry_interval`
- **THEN** the config loads successfully with those fields applied to the root exactly as for a non-composite `query` source
