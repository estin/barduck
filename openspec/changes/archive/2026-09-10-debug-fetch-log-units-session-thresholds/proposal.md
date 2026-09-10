## Why

Three small gaps in the new source model: log-view rows show bare values without units, there is no way to dry-run a source's command without polluting the database, and `jsonl` threshold overrides persist in the database across restarts when they should follow the live session.

## What Changes

- Log view: each entry's value cell renders the value together with the source's unit (same `value + unit` form panels use); sources without a unit render as today.
- New `fetch` CLI command: runs one source's command once and prints the result without writing anything to the database (no readings, logs, health events, or threshold changes). Human-readable table by default, `--json` for JSON. Query sources run once; stream sources print the first parsed lines (up to 5) or until the source timeout, then the command is killed. Execution honors the source's configured timeout. Unknown source names fail naming the source.
- **BREAKING** `jsonl` threshold overrides become session-only: they apply to coloring while the daemon lives and are forgotten on restart, when the config's bands apply again. The `source_thresholds` table is no longer written; existing rows are ignored.

## Capabilities

### New Capabilities

- None.

### Modified Capabilities

- `web-ui`: log view value cells render with the source unit.
- `cli`: new source debug fetch command (ADDED requirement).
- `source-configuration`: `jsonl` threshold overrides are session-scoped, not persisted.
- `data-collection`: override lifetime tied to the daemon session; restart reseeds from config.

## Impact

- Affected code: `src/web/routes.rs` (log view value cell), `src/main.rs` + `src/cli_report.rs` (new command), `src/collector.rs` + `src/source.rs` (oneshot/stream debug run), `src/db.rs` (stop writing `source_thresholds`; read path falls back to declared bands), health payload (effective bands from session state).
- **BREAKING** for anyone relying on overrides surviving a restart; the `source_thresholds` table becomes vestigial (left in schema, ignored).
