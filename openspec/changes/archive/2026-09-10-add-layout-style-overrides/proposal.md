## Why

Barduck web panels currently have no per-layout styling control. All layouts render with the same CSS classes and font settings, making it impossible to optimize the display for different content types — for example, rendering markdown source in a monospace font for code-heavy panels, or adjusting font size for readability. Users need the ability to specify style overrides per layout so that different dashboard sections can be visually tailored to their content.

## What Changes

- Add a `style` field to `LayoutCfg` that accepts CSS property overrides as a TOML inline table
- Propagate layout-level style overrides through to the web panel grid rendering as inline styles
- Update `Cell` variants (`Group`, `Pane`, `Text`) to support per-cell `style` overrides for fine-grained control
- **BREAKING**: The `Cell` enum variant order and field structure changes to accommodate style fields on `Group`, `Pane`, and `Text` variants

## Capabilities

### New Capabilities
- `layout-style-overrides`: Allow layout and cell-level CSS style overrides in the web UI, enabling per-panel font family, font size, and other CSS property customization

### Modified Capabilities
- (none) — existing layout configs without `style` fields continue to work unchanged

## Non-goals

- No changes to the TUI or CLI rendering paths
- No changes to the CSS theme system (`assets/styles.css`)
- No changes to source configuration or storage
- Style overrides apply only to the web UI surface
