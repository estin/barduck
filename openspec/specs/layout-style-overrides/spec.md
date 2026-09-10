## Purpose

Enable per-layout and per-cell CSS style overrides in barduck's web UI, so different dashboard sections can be visually tailored — for example, rendering markdown source in a monospace font or adjusting font size for readability.

## ADDED Requirements

### Requirement: Layouts accept style override configuration

A `layouts` entry MAY include a `style` field that specifies CSS property overrides. The `style` field is an inline table mapping CSS property names to string values (e.g., `font_family = "monospace"`, `font_size = "14px"`). Layouts without a `style` field behave exactly as before.

#### Scenario: Layout with style overrides renders with those styles applied
- **WHEN** a layout has `style = { font_family = "monospace", font_size = "14px" }`
- **THEN** the web panel grid for that layout renders with `font-family: monospace; font-size: 14px;` applied to its container element

#### Scenario: Layout without style overrides renders unchanged
- **WHEN** a layout has no `style` field
- **THEN** the web panel grid renders with default styling (no inline style attribute added)

#### Scenario: Any CSS property can be overridden
- **WHEN** a layout has `style = { background_color = "var(--background)", gap = "8px" }`
- **THEN** the web panel grid renders with those CSS properties as inline styles

### Requirement: Cell variants accept style override configuration

`Group`, `Pane`, and `Text` cell variants MAY include a `style` field that specifies per-cell CSS property overrides. Cells without a `style` field render unchanged.

#### Scenario: Cell with style override renders with that style
- **WHEN** a `Group` cell has `style = { font_family = "monospace" }`
- **THEN** the panel card for that cell renders with `font-family: monospace` applied

#### Scenario: Source cell style override (bare string)
- **WHEN** a bare string source cell (e.g., `"gcal-events"`) has no style field
- **THEN** the panel renders with default styling (no change)

### Requirement: Style overrides compose with existing CSS classes

Layout and cell `style` overrides are applied as inline styles and do not conflict with Tailwind CSS utility classes already present in the rendered HTML.

#### Scenario: Style override does not remove Tailwind classes
- **WHEN** a layout has `style = { font_size = "16px" }` and the panel card uses `class="text-xl"`
- **THEN** both the inline `font-size: 16px` and the `text-xl` class are present; inline style wins per CSS cascade rules

### Requirement: Backward compatibility

All existing layout configurations without `style` fields MUST continue to work identically. The `style` field is optional on all variants.

#### Scenario: Existing config loads without error
- **WHEN** a `config.toml` has `[[layouts]]` with only `title` and `rows` fields
- **THEN** the config loads and renders correctly, identical to before

### Requirement: Style values are escaped for HTML safety

Style property values MUST be escaped when rendered as inline HTML styles to prevent XSS injection.

#### Scenario: Malicious style value is escaped
- **WHEN** a layout has `style = { background_image = "url(javascript:alert(1))" }`
- **THEN** the value is HTML-escaped in the inline style attribute, preventing script execution
