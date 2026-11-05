# Design

## Context

`ValueFormat` is shared by sources and static-text cells. Source values render in single-source and grouped web panels, and in the CLI `latest` table. Stored values and JSON output remain raw.

## Decisions

- `duration` means a non-negative number of seconds. Parse it as a finite number and render it with the project's compact units. Keep invalid or negative values unchanged.
- `elapsed` means an RFC 3339 timestamp. Render its age using the existing precise age format. Keep invalid timestamps unchanged and show future timestamps as `just now`.
- Both formats omit the configured unit because they render their own time units.
- Use the shared display logic for single-source and grouped web values. Use the same formatting rules in the CLI table.
- Leave TUI display unchanged. Leave storage and JSON output unchanged.
- Reuse existing `chrono` and `humantime` dependencies.

## Risks / Trade-offs

- Values that do not match the selected format remain raw. This avoids breaking displays when a source emits unexpected data.
- `elapsed` changes only display output. Each render uses the current wall clock, so the shown age can advance over time.

## Migration Plan

This change adds format values and does not alter existing values. No migration is needed.
