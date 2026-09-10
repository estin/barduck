## Context

See proposal.md (Why) for motivation. Current state shaping the approach:

- `src/main.rs`: `QueryArgs` (flattened `--json`, `--daemon`, repeatable `--source/-s`) already serves `latest`/`logs`; `history`/`health` ride on it. `history` and `fetch` each take a **positional** `source: String`; `fetch` carries its own one-off `--json`.
- `src/cli_report.rs`: `print_history`/`print_health` are CLI-only wrappers over `Backend` methods that the HTTP API also uses (`src/query.rs`, `src/web/routes.rs`).
- `src/config/layout.rs`: `ValueFormat::{Text, Markdown, Json}`, `VALUE_FORMATS` drives the unknown-format error text; `ValueFormat::parse` is the single validation choke point (`src/config/validation.rs`). Rendering is a three-way branch in `src/web/markdown.rs::formatted_content` (JSON = raw-value line + pretty block); the TUI already renders every format as raw text.
- `demo/config.toml` has one `format = "json"` source (`status-json`, emits a JSON object).

## Goals / Non-Goals

**Goals:**
- One flag shape for sources (`--source`/`-s`) and one output switch (`--json`) across every command that prints data.
- Dead CLI surface and dead format variant fully removed, not deprecated.

**Non-Goals:**
- No HTTP API, daemon collection, TUI data-panel, or storage changes.
- No new global CLI flags; `daemon`/`tui` invocation is untouched.

## Decisions

- **Fetch takes a required single `--source`/`-s`, not a repeatable filter.** Fetch runs exactly one source; a `Vec` that silently uses the first would hide user error. Clap enforces presence and reports the missing flag itself (spec scenario: Missing source flag rejected).
- **`history`/`health` removed at the clap layer; `Backend` untouched.** Deleting the `Cmd` variants makes clap itself reject the old names with an unknown-subcommand error — no custom error path. `print_history`/`print_health` are deleted from `cli_report.rs`, but `Backend::history`/`Backend::health` (or equivalents) stay because routes serve them; apply MUST confirm via references before deleting anything below `cli_report`.
- **`reset --json` reuses the per-command flag pattern, not a global flag.** A global `--json` would also parse for `daemon`/`tui` where JSON output is meaningless. `reset` prints a small JSON object (database path + status) instead of the human confirmation line.
- **`format = "json"` rejected by the existing choke point, no new code.** Removing the variant from `ValueFormat::parse` automatically rejects it with the standard unknown-format error (its text updates via `VALUE_FORMATS.join`). `formatted_content` collapses to the two-way markdown/text branch; the now-unused `json_pretty` helper is deleted. Alternative (silent fallback to text) rejected per user decision — loud failure beats quietly dropping the pretty block.
- **Demo `status-json` switches to `format = "markdown"`.** Its value is a JSON object; markdown rendering shows it as a code block-ish literal, closest surviving visual to the old raw-value line. (Alternative `text` renders identically in the TUI; markdown keeps the web panel structured.)

## Risks / Trade-offs

- [Risk] Integration/unit tests invoke `history`, `health`, or positional `fetch` → **Mitigation**: apply updates every caller; `cargo test` is the gate (spec scenarios pin the new shapes).
- [Risk] External scripts pinning the old CLI shapes break silently → **Mitigation**: accepted — breaking changes are spec'd; clap errors name the problem (unknown subcommand / missing flag).
- [Risk] User configs with `format = "json"` fail at startup after upgrade → **Mitigation**: error names the unknown format; migration is one word (`text` or `markdown`).
- [Trade-off] `Backend` keeps methods with no CLI caller — slight dead-surface appearance, but they serve the API; no duplication is introduced.

## Migration Plan

1. Land code + spec sync (this change).
2. No data migration: nothing stored changes.
3. Rollback: revert; old configs and CLI invocations work again (no forward migration to undo).

## Open Questions

None — flag scope and `json`-format compat were decided with the user before planning.
