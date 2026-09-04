## Why

Two independent gaps: (1) deploying barduck (e.g. via the Nix home-manager module) currently means baking every setting into `config.toml` — there is no way to override a value like `listen` per-deployment without templating the file, so environment variables need to take priority over the config file; (2) the layout grid only has two shapes for "more than one value in a pane" — a single source (`Cell::Source`/`Cell::Pane`) or a flat list of table-style rows (`Cell::Group`) — with no way to give one value visual prominence while still listing related values compactly or in a table underneath. Users want a pane that can show a primary value plus supporting values without splitting them into separate panes.

## What Changes

- Top-level `Config` scalar settings (`database_path`, `listen`, `interval`, `failure_threshold`, `stale_after`, `history_points`, `tui_width`) MAY be overridden by a `BARDUCK_<FIELD>` environment variable at startup; when set, the environment variable's value wins over the config file's value (and the config file's own default), using the same parsing rules the field uses in TOML (e.g. humantime for durations).
- The `Cell::Group` layout cell is redesigned into a generalized pane cell with three independent, combinable sections instead of a single flat `ids` list — **BREAKING**:
  - `main` (optional, one member): rendered exactly as today's single-source panel (full-size value, health/threshold-colored border+background+text, always-shown age, its own history bar when the source has threshold bands and doesn't opt out).
  - `secondary` (optional, list of members): rendered like `main` (full value, health-first coloring, always-shown age) but in a smaller font and never with a history bar.
  - `table` (optional, list of members): renders exactly as today's `Group` `ids` rows (label + value, per-row health-first color on the value only, age shown only when stale, own history bar per row).
  - At least one of `main`/`secondary`/`table` must be present; the legacy `ids` field is removed in favor of `table`.
  - The card's border is colored by the worst color across every member in `main`, `secondary`, and `table` combined.
  - The TUI renders the same three sections (no history bars exist in the TUI regardless of section).
- Pane rendering logic in `web.rs`/`tui.rs` is shared across a bare `Cell::Source`/`Cell::Pane` and a group's `main` member, and across a group's `table` members and the current single-list-row renderer, instead of duplicating per-cell-kind rendering code.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `source-configuration`: adds environment-variable overrides for top-level config settings; replaces the `Group` cell's `ids` field with combinable `main`/`secondary`/`table` sections.
- `web-ui`: group pane rendering gains `main` (regular-panel treatment) and `secondary` (compact, no history bar) sections alongside the existing `table` row rendering; card border coloring spans all three sections.
- `tui`: group panel rendering gains the same `main`/`secondary`/`table` sections (no history bars, since the TUI has none).

## Impact

- `src/config.rs`: `Config` gains an env-var override step in `load()`; `Cell::Group` schema changes from `{ title, ids }` to `{ title, main?, secondary?, table? }`; validation extended to require at least one section and to check unknown sources across all three.
- `src/web.rs`, `src/tui.rs`: group-pane rendering split into shared main/secondary/table rendering helpers reused by single-source cells.
- `demo/config.toml`: any example group cell using `ids` updated to `table`.
- Existing user configs using `Cell::Group { ids }` must migrate `ids` to `table` (breaking).
