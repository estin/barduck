## ADDED Requirements

### Requirement: Composite source command output

A composite source's command SHALL be run exactly like any other `query` source's, on its own schedule, with `setup` and `timeout` applying to it as usual (spec: source-configuration — Composite source children). Its stdout MUST parse as a JSON array; each element MUST be shaped like an HTTP ingest payload (spec: http-api — HTTP ingest endpoint): `source` (the full name of one of the composite's declared children), `value`, and optionally `ts` and `thresholds`. Each element SHALL be stored to its named child exactly as if that value had been pushed to `/api/ingest` for it — the same value-type conversion, the same threshold validation, and a fetch log entry with origin `poll`. The composite root itself SHALL NOT store a reading: its own fetch log entry reflects only whether the command ran and its stdout parsed as the described array, never a child's value.

#### Scenario: Well-formed array stores every child's value

- **WHEN** a composite source's command prints a JSON array with one entry per declared child
- **THEN** each child's latest value, timestamp, and thresholds reflect its entry, stored as if pushed via `/api/ingest`

#### Scenario: Malformed output fails the whole attempt

- **WHEN** a composite source's command prints stdout that is not a JSON array, or fails to run
- **THEN** the root's fetch log records the attempt as failed and no child is updated

### Requirement: Composite fan-out error handling

An array entry naming an id that is not one of the composite source's declared children MUST fail the whole attempt: no reading is recorded for any child that tick, and the root's fetch log entry records the failure naming the unrecognized id. A declared child absent from the array SHALL be recorded as a failed attempt for that child alone, with an error noting it was missing from the composite output; the root and every other present child are unaffected.

#### Scenario: Unknown id in the array fails the whole attempt

- **WHEN** a composite source's output array contains an entry whose `source` is not one of its declared children
- **THEN** the root's fetch log records the failure naming the unrecognized id, and no child is updated for that tick

#### Scenario: Missing declared child fails only that child

- **WHEN** a composite source's output array omits one of its declared children while including the others
- **THEN** the missing child's fetch log records a failure noting it was absent, while the root and the present children succeed normally

### Requirement: Force polling a composite source or its children

Forcing a poll of a composite root, or of any one of its children, SHALL run the parent's command exactly once and fan out to every declared child; the composite source's own collector task carries this out, so it cannot overlap that source's scheduled fetch (spec: data-collection — Forced polls are serialized with a source's schedule). While that command is running, the poll-in-progress signal (spec: data-collection — Poll-in-progress is visible) SHALL be visible for the root and for every declared child, not only whichever name the caller requested. The outcome returned to the caller SHALL be scoped to the name they requested: the root's own command/parse outcome when the root was named, or that specific child's resulting value when a child was named.

#### Scenario: Forcing the root fans out to all children

- **WHEN** a composite root is force-polled
- **THEN** its command runs once and every declared child's value is refreshed from that single run

#### Scenario: Forcing one child fans out to all its siblings

- **WHEN** one child of a composite source is force-polled
- **THEN** the parent's command runs once and every declared child — including siblings not named in the request — is refreshed from that run

#### Scenario: Root and every child show polling together

- **WHEN** a composite source's command is running, whether triggered by its schedule or by forcing the root or any one child
- **THEN** the root and every declared child are reported as currently polling for the duration of that one command

#### Scenario: Forcing a child returns that child's own outcome

- **WHEN** a specific child is force-polled
- **THEN** the returned outcome describes that child's resulting value, not the whole batch or the root's outcome
