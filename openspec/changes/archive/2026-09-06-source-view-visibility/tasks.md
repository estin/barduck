## 1. Config schema

- [x] 1.1 Add a `show_in` field to `SourceCfg` in `src/config.rs` (optional enum-like value: `"all"` (default), `"tui"`, `"web"`), and verify existing configs with no `show_in` field still deserialize with the default (spec: source-configuration — Per-source view visibility, "Default is visible everywhere")
- [x] 1.2 Reject any other `show_in` value at startup, naming the source and the invalid value, and verify with a unit test asserting the error message names both (spec: source-configuration — Per-source view visibility, "Invalid value rejected")
- [x] 1.3 Verify `show_in` is read but never consulted by the collector/scheduler by grepping the collection code path and confirming no reference to it, keeping fetch scheduling unaffected (spec: source-configuration — Per-source view visibility, "Collection unaffected by view restriction")

## 2. Shared view-visibility resolution

- [x] 2.1 Add a small helper (used by both `src/tui.rs` and `src/web.rs`) that, given a source id and a view name (`"tui"` or `"web"`), returns whether that source is visible in that view per its `show_in` value, and cover it with unit tests for all three `show_in` values against both views
- [x] 2.2 Add a helper that, given a generalized pane cell's `main`/`secondary`/`table` members and a view name, returns the subset of members visible in that view, and cover it with a unit test for the "all members hidden" case returning an empty result

## 3. TUI rendering

- [x] 3.1 In the TUI's grid-building code, substitute a space cell (same column span) for a bare source or `{ id, title }` cell whose source is hidden from `"tui"`, and verify with a rendering test that the panel is replaced by an empty region (spec: tui — Hidden sources render as space in the TUI, "Cell hidden from the TUI renders as space")
- [x] 3.2 In the TUI's generalized-pane rendering, omit hidden members from `main`/`secondary`/`table` and collapse the whole cell to a space when no members remain visible, and verify with tests covering partial-hide and all-hidden cases (spec: tui — Hidden sources render as space in the TUI, "Hidden generalized pane member is omitted" and "Generalized pane cell with every member hidden renders as space")
- [x] 3.3 Verify column count and row geometry are computed identically whether or not any source is hidden, via a test comparing layout dimensions with and without a `show_in` restriction (spec: tui — Hidden sources render as space in the TUI)

## 4. Web UI rendering

- [x] 4.1 In the web UI's grid-building code, render an empty grid position (same column span) for a bare source or `{ id, title }` cell whose source is hidden from `"web"`, and verify with a test asserting no card markup is emitted for that position (spec: web-ui — Hidden sources render as space in the web dashboard, "Cell hidden from the web UI renders empty")
- [x] 4.2 In the web UI's generalized-pane rendering, omit hidden members and collapse the cell to empty when no members remain visible, mirroring task 3.2 (spec: web-ui — Hidden sources render as space in the web dashboard, "Hidden generalized pane member is omitted" and "Generalized pane cell with every member hidden renders empty")
- [x] 4.3 Exclude a source hidden from `"web"` from the source summary strip's chip list, and verify with a test that the chip is absent while chips for visible sources keep their existing order (spec: web-ui — Hidden sources render as space in the web dashboard)

## 5. Cross-view integration check

- [x] 5.1 Add an integration test (in `tests/integration.rs` or the demo config) with one source set to `show_in = "tui"` and another to `show_in = "web"`, asserting each source appears only in its designated view's rendered output from the same shared layout config, verifying the "Same layout renders differently per view" scenario in both the tui and web-ui specs
- [x] 5.2 Update `demo/config.toml` (and/or `demo/README.md`) with a short example of `show_in`, and verify the demo config still passes startup validation
