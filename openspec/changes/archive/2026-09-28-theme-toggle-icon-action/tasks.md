# Tasks

## 1. Toggle button markup

- [x] 1.1 Swap the sun/moon SVG visibility classes in `page_chrome` (`src/web/routes.rs`) so the button shows the target theme's icon, and verify by rendering/inspecting both themes (dark `<html>` shows sun, light shows moon)
- [x] 1.2 Render `aria-label` (and matching `title`) server-side from the theme value (`dark` → "Switch to light theme", otherwise → "Switch to dark theme"), and verify the label matches the shown icon on first paint for both themes

## 2. Regression coverage

- [x] 2.1 Update/extend `tests/integration.rs` toggle expectations (icon visibility classes, per-theme accessible label) and verify with `cargo test` for the affected tests
- [x] 2.2 Run `cargo clippy` and the full relevant test suite to verify no regressions
