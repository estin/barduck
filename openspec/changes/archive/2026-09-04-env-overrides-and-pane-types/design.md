## Context

`Config` (`src/config.rs`) is loaded once from TOML via `toml::from_str` in `config::load()`, then validated. `Cell::Group { title, ids: Vec<GroupItem> }` is one variant of the untagged `Cell` enum; both `src/web.rs` and `src/tui.rs` pattern-match on `Cell::Group { title, ids }` (`src/web.rs:334`, `src/tui.rs:155`) and render every member as a "table row" (label + value, age-only-when-stale, own history bar). A bare `Cell::Source`/`Cell::Pane` is rendered by a separate, single-source code path in each UI. There is currently no environment-variable layer between the TOML file and `Config`. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**
- Let a small, fixed set of top-level settings be overridden by `BARDUCK_*` env vars without touching `sources`/`layouts`.
- Replace `Cell::Group`'s flat `ids` with `main`/`secondary`/`table`, sharing rendering code with the existing single-source and table-row paths rather than writing a third, parallel implementation.

**Non-Goals:**
- No generic/recursive nesting of panes inside panes — `main`/`secondary`/`table` are a fixed, flat three-section shape, not an arbitrary tree.
- No env var support for `sources`/`layouts` (structural, not scalar — templating those is out of scope).
- No config migration tool — `ids` → `table` is a manual, one-line rename users make themselves (small, single-user configs).

## Decisions

**Env var overrides applied as a post-parse patch, not serde field attributes.** After `toml::from_str` produces a `Config`, a small function walks the seven named fields, and for each one whose `BARDUCK_<FIELD>` var is set, parses the var's string with that field's existing scalar parser (humantime for `interval`/`stale_after`, `str::parse` for integers, the existing `TuiWidth` string-or-int logic for `tui_width`) and overwrites the field. Alternative considered: a serde `deserialize_with` per field that checks the env first — rejected because serde has no clean way to say "environment beats the file's explicit value but not its own default" from inside a field deserializer without duplicating every default function's logic there instead of in one pass over an already-built `Config`.

**`main`/`secondary`/`table` reuse `GroupItem` as the member type.** `GroupItem` (bare string or `{id, label}`) is unchanged; `Cell::Group` becomes `Group { title: Option<String>, main: Option<GroupItem>, secondary: Vec<GroupItem>, table: Vec<GroupItem> }` (`title` and `main` default to `None`, `secondary`/`table` to empty, all via `#[serde(default)]` — a cell needs none of them individually). `Cell::source_names()` collects from all three; `Cell::pane_title()` returns `title.as_deref()`, `None` when omitted. Validation rejects a `Group` where `main.is_none() && secondary.is_empty() && table.is_empty()`, and rejects an explicitly empty `title = ""` (an absent `title` is fine — rendering falls back to `main`'s own label, or no header text when there's no `main` either).

**Rendering shares helpers instead of branching three ways.** In both `web.rs` and `tui.rs`, the existing single-source panel renderer is extracted (if not already a standalone function) into a helper taking a source id + display label + "compact" flag; `Cell::Source`/`Cell::Pane` call it with `compact = false`, a `Group`'s `main` calls it the same way, and a `Group`'s `secondary` members call it with `compact = true` (web: smaller font class, no history bar; tui: non-emphasized style). The existing table-row renderer (today's whole `Group` body) is reused unchanged for `table` members. The card/panel's own border-color computation folds over `main.iter().chain(&secondary).chain(&table)` instead of just `ids`.

**No change to `GroupItem`'s untagged bare-string-or-table shape.** Keeps `main = "cpu-load"` and `table = ["a", {id="b", label="B"}]` both valid, matching the existing `ids` ergonomics users already know.

## Risks / Trade-offs

- [Breaking config change] Any existing config using `{ title, ids }` fails validation after this change (unknown field `ids` under `deny_unknown_fields`) → Document the one-line `ids` → `table` rename in the proposal/changelog; `demo/config.toml` is updated as the reference example.
- [Compact-style drift between web and TUI] "Smaller font" (web) and "non-emphasized style" (TUI) are different mechanisms for the same intent → both are spec'd explicitly per-surface (see the `web-ui` and `tui` delta specs) so they don't need to look identical, only serve the same "less prominent than main" purpose.
- [Env var parse errors surface at startup, not at the point of use] A malformed `BARDUCK_STALE_AFTER` fails the whole daemon start rather than falling back silently → matches existing config-validation behavior (bad TOML also fails startup), so no new failure mode class is introduced.

## Migration Plan

1. Add the env-var override pass to `config::load()` (applied after parse+before `validate`, so an env-supplied bad value still hits the same `validate()` checks as a file value where applicable, e.g. `history_points > 0`).
2. Change `Cell::Group`'s shape and `Cell::source_names`/`Cell::pane_title` accordingly; update `validate_cell` for the new empty-pane check.
3. Refactor `web.rs`/`tui.rs` group rendering into shared main/secondary/table sections per the Decisions above.
4. Update `demo/config.toml` and `demo/README.md` if either uses `ids` today.
5. Update existing tests referencing `Cell::Group { title, ids }` (`src/tui.rs` `group_panel_*` tests, any equivalent in `src/web.rs`/`tests/integration.rs`) to the new shape, and add new tests for `main`/`secondary` per the added scenarios.

No runtime/data migration needed — this only affects config-file parsing and in-memory rendering; the database schema and collected readings are untouched.
