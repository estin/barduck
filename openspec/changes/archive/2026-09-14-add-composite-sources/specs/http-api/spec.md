## ADDED Requirements

### Requirement: Force poll endpoint accepts composite roots and children

`POST /api/sources/{source_name}/poll` (spec: http-api — Force poll endpoint) SHALL accept a composite source's own name or one of its children's full name, with the same single-command fan-out described in data-collection (spec: data-collection — Force polling a composite source or its children). The response is the poll outcome for the requested name only — the root's command/parse outcome, or the named child's resulting value.

#### Scenario: POST to a composite root's poll path

- **WHEN** a client sends `POST /api/sources/load/poll` for a composite source named `load`
- **THEN** the parent's command runs once, every declared child is refreshed, and the response describes the root's command/parse outcome

#### Scenario: POST to a child's poll path

- **WHEN** a client sends `POST /api/sources/load::1m/poll`
- **THEN** the parent's command runs once, every declared child is refreshed, and the response describes `load::1m`'s resulting value
