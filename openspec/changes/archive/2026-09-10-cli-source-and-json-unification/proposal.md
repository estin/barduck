## Why

The CLI grew organically: `history` takes a positional source while `latest`/`logs` take `--source`, `fetch` takes a positional source with its own one-off `--json` flag, and `reset` has no machine-readable output at all. Two query surfaces (`history`, `health`) duplicate what the web UI, TUI, and HTTP API already serve. Separately, the `json` value format (pretty-printed JSON block under a source's value) is a rendering special-case with no consumers asking for it. Unifying the CLI surface and removing the dead format now, before more tooling pins the current shapes.

## What Changes

- **BREAKING**: Remove the `history` and `health` CLI subcommands (HTTP API, web UI, and TUI history/health surfaces are unchanged).
- **BREAKING**: `fetch` takes its source as `--source <name>` / `-s <name>` (required) instead of a positional argument.
- `reset` gains `--json` (machine-readable result); every remaining data-producing command (`latest`, `logs`, `fetch`) already supports `--json`, so all CLI commands with observable output support `--json` after this change.
- **BREAKING**: Remove `ValueFormat::Json`. `format = "json"` on a source or static-text cell is rejected at config load like any other unknown format; the web UI's pretty-printed JSON block goes away with it.
- Update the demo config's `format = "json"` example to a surviving format.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `cli`: command set shrinks to data commands with uniform `--source`/`--json` flags; `reset` gains `--json`.
- `source-configuration`: `format` accepts only `text` (default) and `markdown`; `json` is rejected.
- `web-ui`: value/static-text rendering drops the pretty-printed JSON block.
- `tui`: static-text wording drops the `json` example (TUI already renders all formats as raw text).

## Impact

- `src/main.rs` (clap command definitions + dispatch), `src/cli_report.rs` (`print_history`/`print_health` removed), tests invoking the removed subcommands.
- `src/config/layout.rs` (`ValueFormat::Json` variant, `VALUE_FORMATS`), `src/config/validation.rs` (unchanged logic, new accepted set), `src/web/markdown.rs` (`formatted_content` Json branch), `src/web/panels.rs` (format plumbing), `demo/config.toml` (`format = "json"` example).
- HTTP API endpoints, daemon collection, TUI data panels, and stored data are unaffected.
