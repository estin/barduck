## 1. Light/dark theme toggle

**Revised during implementation** (see the paused-and-resumed discussion): rather than a client-only `localStorage` + blocking-script bootstrap, the theme choice is persisted server-side in a `bd_theme` cookie (`POST /api/theme`, `src/api.rs`), and every page reads it (`theme_class(cx)`, `src/web.rs`) to render the correct `dark`/`light` class on `<html>` directly — "the backend knows the user's theme". This fully avoids any flash of the wrong theme with no bootstrap script needed; a first-time visitor with no cookie yet still gets their OS preference via a `prefers-color-scheme` CSS media query in `assets/styles.css`. Status colors (panel borders/backgrounds/text, summary-strip chips) were also moved from hardcoded light-mode hex values to `--status-*` CSS custom properties (light values in `:root`, dark overrides in `.dark`/the media query) so they adapt to the theme too — this wasn't in the original plan but was necessary for the toggle to actually look right in dark mode.

- [x] 1.1 Add `.cookies()` to the router builder (`src/lib.rs`); add `POST /api/theme` (`src/api.rs`) validating `{"theme": "dark"|"light"}` and writing the `bd_theme` cookie; add `theme_class(cx)` (`src/web.rs`) reading it back and applied via `<html class=(theme)>` on both `dashboard()` and `source_logs()`; add the `prefers-color-scheme` media query (guarded by `:not(.light)`) to `assets/styles.css` for the no-cookie-yet case
- [x] 1.2 Add a theme toggle button (`id="bd-theme-toggle"`, sun/moon SVGs swapped via `dark:` classes) near the connection indicator in the header; its click handler (`THEME_TOGGLE_SCRIPT`) flips `<html>`'s `dark` class immediately and POSTs the new value to `/api/theme` to persist it; verified via an integration test that the button exists and that the cookie round-trip actually changes what the server renders
- [x] 1.3 Extend `full_style_for_color`/`border_style_for_color`/`text_style_for_color`/`chip_style` (`src/web.rs`) to reference `var(--status-*)` tokens instead of literal hex, so status coloring adapts to the active theme; add the corresponding `--status-*` tokens (light + dark) to `assets/styles.css`

## 2. Panel title on the card border

- [x] 2.1 Replace `card_header(card_title(...))` in both `panels_grid` card-rendering branches (solo-panel, combined multi-section) with a `relative`-positioned wrapper and an absolutely positioned title `<span>` over the card's top border, padded left/right
- [x] 2.2 Make the title span's background match the card's own current background: reuse `Panel::status_style()`'s inline style for a solo panel's span; use the neutral `bg-background` class (no per-status override) for a combined card's span
- [x] 2.3 Render no title span at all when a generalized pane cell has no `title` and no `main` (matching "no header text" from before this change)
- [x] 2.4 Add/update integration tests: a single-source panel's title renders as a border span (not inside a separate header `<div>`), a generalized pane's title does the same, and an untitled/no-main pane renders no title span

## 3. Compact panel density

- [x] 3.1 Reduce padding/gap constants in `src/components/card.rs` (`CARD`, `card_header`, `card_content`, `card_footer`) for a denser default card
- [x] 3.2 Reduce non-conflicting spacing/type-scale classes set directly in `web.rs`'s panel markup (grid `gap`, value text size, row padding) to match the denser density
- [x] 3.3 Verify via integration test that a panel rendered after this change still shows its value, unit, status label, "updated X ago" text, and history bar — nothing is hidden, only the spacing/type scale changed

## 4. Responsive layout for small viewports

- [x] 4.1 Add `<meta name="viewport" content="width=device-width, initial-scale=1">` to `dashboard()`'s `<head>`
- [x] 4.2 Give the panel grid container a stable `bd-panel-grid` class and each cell/slot wrapper a stable `bd-panel-cell` class, alongside their existing dynamic inline `grid-template-columns`/`grid-row`/`grid-column` styles
- [x] 4.3 Add a `<style>` block (alongside the existing `[id^='panel-']:target` one) with a `max-width: 640px` media query that forces `.bd-panel-grid` to one column and resets `.bd-panel-cell`'s row/column placement to `auto`, both with `!important`, and a code comment explaining why `!important` is needed here
- [x] 4.4 Add/update integration tests: the viewport meta tag is present, and the grid/cell classes (`bd-panel-grid`/`bd-panel-cell`) are present on the rendered elements

## 5. Validation

- [x] 5.1 Run `cargo build`, `cargo clippy --all-targets`, and `cargo nextest run`; confirm no new failures beyond the pre-existing sandbox-only TCP test failures

## 6. Follow-up polish (requested after initial review)

- [x] 6.1 Make summary-strip chips more compact: replaced the `button_variants(Outline, Sm)` styling (a fixed `h-8` button) with a small pill (`rounded-full px-2 py-0.5 text-xs leading-none`, no fixed height) in `panels_grid`; reduced the chip row's own gap/margin (`gap-2 mb-6` → `gap-1.5 mb-4`)
- [x] 6.2 Center the border-embedded title vertically on the border line: replaced the fixed `-top-2.5` offset (which only happened to roughly line up for one particular line-height) with `top-0 -translate-y-1/2` on both title-span call sites, which centers the span on the border line regardless of font metrics
