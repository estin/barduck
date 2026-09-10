## 1. Config Model

- [x] 1.1 Add `style: Option<HashMap<String, String>>` field to `LayoutCfg` in `src/config/layout.rs` with `#[serde(rename_all = "kebab-case")]` and `#[serde(default)]`
- [x] 1.2 Add `style: Option<HashMap<String, String>>` field to `Cell::Pane`, `Cell::Group`, and `Cell::Text` variants
- [x] 1.3 Add `use std::collections::HashMap;` import to `src/config/layout.rs`
- [x] 1.4 Ensure `Cell` enum variant order preserves deserialization for cells without `style` (keep `Text` before `Group`)

## 2. Config Validation

- [x] 2.1 Update `validate_cell` in `src/config/validation.rs` to match the new `Pane`, `Group`, and `Text` variant fields (include `style` in match patterns)
- [x] 2.2 Verify `cargo test` passes for config validation tests

## 3. Web Panel Rendering

- [x] 3.1 Apply layout-level `style` overrides as inline `style` attribute on the grid container in `src/web/panels.rs` (`panels_grid` function)
- [x] 3.2 Apply cell-level `style` overrides as inline `style` attribute on panel cards in `src/web/panels.rs` (`panels_grid` function)
- [x] 3.3 Handle `TextPanel` style overrides (removed redundant `TextPanel.style`, using `Slot.style` instead)
- [x] 3.4 Verify existing panels without `style` fields render unchanged (no empty style attributes)

## 4. Build & Test

- [x] 4.1 `cargo build` passes with no warnings
- [x] 4.2 `cargo test` passes (all 37 existing + 2 new tests)
- [x] 4.3 Verify config parsing: `config.toml` with `style = { font_family = "monospace" }` loads correctly
- [x] 4.4 Verify config parsing: existing `config.toml` without `style` fields loads unchanged
