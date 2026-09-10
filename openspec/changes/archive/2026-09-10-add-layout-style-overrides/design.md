## Context

Barduck's web UI renders dashboard layouts from `Config.layouts` (a `Vec<LayoutCfg>`). Each `LayoutCfg` has `title` and `rows: Vec<Vec<Cell>>`. The web panel grid is rendered in `src/web/panels.rs` by `panels_grid()`, which iterates over layouts and cells, producing Tailwind CSS class-based HTML. Layout styles are not configurable — all layouts use the same default styling.

See proposal.md - Why for the motivation.

## Goals / Non-Goals

**Goals:**
- Add a `style` field to `LayoutCfg` accepting CSS property overrides as a TOML inline table
- Add `style` fields to `Cell` variants (`Group`, `Pane`, `Text`) for per-cell overrides
- Propagate style overrides as inline `style` attributes in the rendered HTML
- Maintain backward compatibility — all existing configs work unchanged

**Non-Goals:**
- No changes to the CSS theme system (`assets/styles.css`)
- No changes to the TUI or CLI rendering paths
- No changes to source configuration or storage
- No CSS framework additions — inline styles only

## Decisions

**Represent `style` as `HashMap<String, String>`**

The `style` field maps CSS property names (kebab-case) to values. Using `HashMap<String, String>` avoids a predefined property list and allows any CSS property. Serialized as a TOML inline table: `style = { font_family = "monospace" }`.

**Apply styles as inline `style` attributes**

Style overrides are rendered as inline `style` HTML attributes on the grid container (layout-level) and panel cards (cell-level). This follows the existing pattern in `panels.rs` where `full_style_for_color()` and `border_style_for_color()` already use inline styles for dynamic CSS (spec: web-ui — light/dark theme toggle).

**Variant order in `Cell` enum**

The `style` field must be added to `Group`, `Pane`, and `Text` variants. Since `Cell` uses `#[serde(untagged)]`, variant order matters. The `Text` variant already has `text: String` as its required field and must remain first to maintain correct deserialization. `Pane` and `Group` get a new `style: Option<HashMap<String, String>>` field placed after their existing fields. This preserves the existing deserialization behavior for cells without `style`.

**Serialization of kebab-case CSS properties**

Rust structs use snake_case but CSS uses kebab-case. We use `#[serde(rename_all = "kebab-case")]` on the style field so `font_family` in Rust serializes as `font-family` in TOML.

**XSS protection**

Style values are rendered through Topcoat's HTML escaping mechanisms. Since `topcoat::view` component macros escape attribute values, style values are automatically escaped when interpolated into `style=(...)` attributes.

## Risks / Trade-offs

- **Untagged enum complexity**: Adding fields to `Cell` variants requires careful handling of the `#[serde(untagged)]` ordering. New variants with required fields must be placed before `Group` in the enum. This follows the established pattern (see the `Text` variant placement note in `layout.rs`).
- **Inline style proliferation**: Adding inline `style` attributes increases HTML payload size slightly. This is negligible for a dashboard with a handful of panels.
- **CSS cascade conflicts**: Inline styles override Tailwind classes per CSS specificity rules. Users may unintentionally override intended styles. This is by design — the user explicitly chose the override.
