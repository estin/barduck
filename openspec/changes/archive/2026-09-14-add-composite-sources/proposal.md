## Why

Some data is naturally a small family of related numbers produced by one cheap command — load averages for 1/5/15 minutes, per-partition disk usage, multi-sensor readings. Today each number needs its own `[[sources]]` entry with its own scheduled command, multiplying process spawns and duplicating scheduling/health bookkeeping for values that are always fetched together anyway. Letting one command's output fan out into several independently-displayed, independently-healthed values removes that duplication.

## What Changes

- A `query` source may declare `children`: named sub-sources that share the parent's command, schedule (`interval`/`cron`), `setup`, and `timeout`. Each child is addressed as `<parent>::<child>` (e.g. `load::1m`) and may declare its own `title`, `unit`, `format`, `thresholds`, `show_history`, `show_in`, `value_type`, and `history_points` — it MUST NOT declare its own `command`, `interval`/`cron`, `timeout`, `setup`, or `retry_interval`.
- A source with `children` (a **composite source**) changes what its command's stdout must be: instead of a single value or a `jsonl` row, it MUST parse as a JSON array whose elements are shaped like an ingest payload (`source`, `value`, optional `ts`, optional `thresholds`) with `source` naming one of the declared children's full id. Each array entry is stored to that child exactly as a pushed/fetched value would be. The composite root itself never stores a scalar reading — only whether its command ran and its output parsed as valid JSON.
- A composite root's own `unit`, `thresholds`, `value_type`, `format`, and `show_history` fields are rejected at config load: they would never apply to a value the root itself doesn't produce. `title` and `show_in` remain valid on the root (they govern the auto-rendered pane described below).
- Force-polling either the composite root or any one of its children runs the parent's command once and fans the result out to every declared child — `poll --source load` and `poll --source load::1m` both trigger the same single command execution; they differ only in whose resulting outcome is returned. All of {root, every child} show the shared "poll in progress" signal for the duration of that one command.
- Fan-out error handling: a declared child missing from the array is recorded as a failed attempt for that child alone (root and sibling children are unaffected). An array entry naming an id that isn't one of the root's declared children fails the whole attempt — nothing is written for that tick — since it signals a misconfigured or drifted script.
- Web UI and TUI: a layout cell that names a composite root directly renders as a combined table of all its children's current values, in declared order, instead of failing as if the root had no value of its own. Children remain ordinary sources individually referenceable in any existing layout cell (`main`, `secondary`, `table`) too.

## Capabilities

### New Capabilities

(none — this extends existing capabilities)

### Modified Capabilities

- `source-configuration`: adds child source declaration, the composite root's command/output contract, and validation of the composite shape (field restrictions on the root, unique full names, non-empty `children`).
- `data-collection`: adds composite fetch/fan-out semantics (JSON-array parsing, per-child storage, missing/unknown-entry error handling) and extends forced polling to cascade from a composite root or any one child to the whole family via a single command execution.
- `cli`: `poll`/`fetch` accept a composite root's name or a child's full name.
- `http-api`: the poll endpoint accepts a composite root's name or a child's full name, with the same fan-out.
- `web-ui`: a layout cell naming a composite root auto-renders as a table of its children.
- `tui`: a layout cell naming a composite root auto-renders as a table of its children.

## Impact

- `src/config/source.rs`: new child-source config shape, composite validation, config-load-time expansion of declared children into addressable sources.
- `src/config/mod.rs` (or equivalent load path): name-uniqueness validation now covers expanded child names; layout reference validation resolves composite-root and child names alike.
- `src/collector.rs`: composite fetch path (run once, parse JSON array, store per child), force-poll control routing so a root or child name reaches the same collector task and fans out.
- `src/health.rs`: unaffected in logic, but every child must resolve to a schedule/timeout for staleness purposes without its own `interval`/`timeout` fields.
- `src/api.rs`, `src/cli_report.rs`, `src/main.rs`: poll/fetch validation extended to composite roots and children (mostly falls out of children being ordinary addressable sources).
- `src/web/panels.rs`, `src/web/routes.rs`, `src/tui.rs`: auto-rendering of a composite root's children as a table pane.
- `tests/integration.rs`, `README.md`, `SKILL.md`: coverage and documentation for the new source shape.
