## Why

The web dashboard already ships a full light/dark design-token pair (`assets/styles.css`'s `:root`/`.dark`) but nothing ever applies the `.dark` class, so the page is permanently light regardless of the viewer's system preference. Separately, the panel grid was designed for desktop only: it forces a fixed CSS `grid-template-columns` column count regardless of viewport width, has no `<meta name="viewport">` tag, and its cards use generous desktop spacing — so on a phone the layout is cramped, sideways-scrolling, or both. And now that panes can combine `main`/`secondary`/`table` sections in one card, the plain header-row title (`card_header(card_title(...))`) takes up a full row that a border-embedded title (as the TUI's `Block::title()` already does) would reclaim.

## What Changes

- Add a light/dark theme toggle button to the web dashboard header. On first visit (no stored preference) the page follows the browser's `prefers-color-scheme`; toggling sets an explicit preference, persisted server-side in a cookie, that overrides the system setting on every later visit — the server renders the correct theme directly from the cookie, so there is no flash of the wrong theme and no client-side bootstrap step. Status colors (panel borders/backgrounds/text, summary-strip chips), previously hardcoded light-mode hex values, move to theme-aware tokens so they render correctly in both themes.
- Tighten the web dashboard's visual density: reduced padding/gaps on cards, rows, and the page shell, and a smaller base type scale for panel content — all currently-shown information (age text, status labels, history bars, footer) stays visible, just denser.
- Move each pane card's title from a `card_header` row into the card's own top border, top-left, with padding on either side — the same visual convention as the TUI's bordered panel title — reclaiming the vertical space a full header row cost.
- Make the web dashboard responsive down to small (phone-width) viewports: add a `<meta name="viewport">` tag, make the panel grid reflow to fewer columns (down to one) as the viewport narrows instead of forcing a fixed column count, and ensure no element causes horizontal overflow at small widths.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `web-ui`: adds the light/dark toggle and its persistence behavior; changes pane card layout (density, border-embedded title) and grid responsiveness for small viewports.

## Impact

- `assets/styles.css`: adds `--status-*` status-color tokens (light/dark) alongside the existing base tokens, plus a `prefers-color-scheme` media query for a first-time visitor with no theme cookie yet, and the narrow-viewport grid override.
- `src/web.rs`: theme-toggle button + click script in `dashboard()`; a `theme_class(cx)` helper applied to `<html>` on both `dashboard()` and `source_logs()`; panel/card markup reworked for the border-embedded title and tighter spacing; the four status-coloring functions emit theme-token `var()` references instead of literal hex; grid/cell elements get stable classes for the narrow-viewport CSS override (the dynamic per-layout inline styles stay, since Tailwind can't generate a class for a runtime column count).
- `src/api.rs`: a `POST /api/theme` route persisting the chosen theme in a cookie (`bd_theme`) — the mechanism behind "the backend knows the user's theme".
- `src/lib.rs`: enables `.cookies()` on the router builder.
- `src/components/card.rs` (vendored `topcoat` component, tracked in `components.toml`): padding/gap constants reduced for density; `card_header`/`card_title` are no longer used by `panels_grid` (replaced by the border-title span) but remain available as generic primitives.
- No changes to the TUI, CLI, or config schema — this change is scoped entirely to the web dashboard's presentation.
