## Context

`draw()` in `src/tui.rs` currently does:
```rust
let row_areas = Layout::vertical(vec![Constraint::Fill(1); rows.len()]).split(body);
```
— `body`'s height is divided evenly across every row regardless of how many there are, so N rows on a short terminal each get `body.height / N`, however small. `panel_widget()` renders each cell's `Paragraph` (bordered, with `Wrap` for the single-panel/text cases) into whatever `Rect` it's given; it has no awareness of whether that `Rect` is tall enough for its content.

`UiState` (`src/tui.rs:158`) currently holds only `error` and `rows`. The event loop (`event_loop`) polls for one crossterm event per frame and only acts on it via `quit_requested`; every other key is dropped.

`state.rows` is a flat `Vec<Vec<Slot>>` — one `Slot` per grid cell, one inner `Vec` per configured row, built fresh from config + latest data on every refresh (`apply_outcome`). There is no existing concept of a row's "content height."

## Goals / Non-Goals

**Goals:**
- When configured rows don't fit the terminal, let the user reach all of them via scrolling, without changing anything for the already-fits case.
- Keep the change local to `src/tui.rs` — no config schema change, no change to how panels/layouts are computed.
- Reuse ratatui's built-in scrollbar rather than hand-rolling one (already available in ratatui 0.30, no new dependency).

**Non-Goals:**
- Horizontal scrolling — `tui_width`/content centering already caps width; only vertical overflow is in scope.
- Scrolling within a single panel's own content (e.g. a very long markdown text panel) — this change scrolls whole grid *rows*, not text inside one panel.
- Persisting scroll position across a resize or across app restarts — a resize is rare enough mid-session that resetting (or just re-clamping) the offset is fine.
- Changing row height computation for the fits-on-screen case — those rows keep stretching via `Constraint::Fill(1)` exactly as today (see proposal.md's note on avoiding a visual regression).

## Decisions

**1. Scroll granularity is whole grid rows, not text lines.**
A "row" here is one entry of `state.rows` (one horizontal strip of the configured grid), matching the unit `draw()` already lays out with `Layout::vertical`. Scrolling by row (rather than by individual terminal line) keeps the implementation to "which slice of `rows` is visible" instead of having to split a single row's rendered `Rect` mid-panel, which `ratatui`'s block/border widgets don't support cleanly. `PgUp`/`PgDn` move by "as many rows as fit in one screen" (computed from the same natural-height numbers used for layout), not a fixed row count, so paging still makes sense whether panels are one line tall or ten.

*Alternative considered*: line-level (terminal-row) scrolling of the whole rendered buffer. Rejected — would need to render into an off-screen buffer sized to full content height and blit a window of it, which ratatui doesn't support out of the box for widget-based rendering (only `Paragraph` has line-level `scroll()`), and mixing that with per-cell bordered panels is significantly more code for no behavior the proposal asks for.

**2. Row natural height = a per-row minimum computed from its cells' content, mirroring what `panel_widget` will render.**
For a row, natural height = 2 (top/bottom border) + the tallest cell's content-line count in that row, with a floor of 3 (so an empty/near-empty panel still shows its border). Content-line count per cell:
- static-text slot: `text.lines().count()`
- single-panel slot (`main` only): `value.lines().count()`
- group-pane slot: `(main present as 1 line) + secondary.len() + table.len()`
- spacer slot (nothing set): 0 (doesn't constrain the row)

This mirrors the line-construction `panel_widget` already does (`main_or_secondary_line`/`table_line`, one `Line` per member) — reusing that shape means the computed height and the actual rendered content agree, rather than guessing a fixed constant that's wrong for tall group panes. This is a lower bound, not a wrap-aware exact height: a value/text line that's wider than the panel and wraps to two visual lines can still render slightly clipped at the very bottom of its row, exactly as today's behavior when a row is too short — this change fixes *rows that don't fit the screen*, not *individual long lines wrapping within a panel*, which is unchanged and out of scope (see Non-Goals).

*Alternative considered*: a single fixed height (e.g. every row is 5 lines). Rejected — group panes with many table rows would still get clipped the same way `Fill(1)` clips them today; the whole point is sizing rows to what they actually need.

**3. Two layout modes in `draw()`, chosen by comparing total natural height to available body height.**
- `total_natural <= body.height`: unchanged — `Layout::vertical(vec![Constraint::Fill(1); rows.len()])`, exactly today's code path. Zero behavior change for every currently-working config.
- `total_natural > body.height`: switch to `Constraint::Length(natural_height[i])` for the visible rows only, i.e. slice `rows[scroll_offset..]` and take rows while their cumulative height fits `body.height`, then lay those out with `Layout::vertical`. A trailing gap (last visible row's bottom to `body`'s bottom) is left blank rather than stretched, which is what makes "which rows are fully visible" well-defined for clamping.

**4. Scroll state lives on `UiState` as a single `scroll_offset: usize` (index into `state.rows`), clamped after every refresh and every scroll key.**
Clamping happens in one place — right before `draw()` is called — using the just-recomputed natural heights and current terminal size, so a resize or a data refresh that changes row count/content never leaves `scroll_offset` pointing past the end. The clamp for "don't scroll past the point where the last row is fully visible" walks backward from the last row accumulating natural heights until adding one more would exceed `body.height`; that walk-back index is `max_offset`. `scroll_offset = scroll_offset.min(max_offset)`.

**5. Keybindings added to the existing key-read branch in `event_loop`, gated on nothing (they're simply no-ops — clamped to `max_offset == 0` — when content already fits).**
`↑`/`k` → `scroll_offset -= 1` (saturating), `↓`/`j` → `+= 1` then clamp, `PgUp`/`PgDn` → same by "rows per screen" (count of rows the current viewport shows), `Home`/`g` → `0`, `End`/`G` → `max_offset`. This sits next to `quit_requested` in the same `crossterm::event::read()` match rather than a new polling path — one event source, same as today.

**6. Scroll indicator: ratatui's `widgets::{Scrollbar, ScrollbarState}` on the right edge of the content area, rendered only when `total_natural > body.height`.**
`ScrollbarState::new(rows.len()).position(scroll_offset)` — row-granularity position, matching the row-granularity scrolling above. This is a well-known, already-vendored widget (no new dependency), so it costs one `render_widget` call rather than hand-drawn characters.

## Risks / Trade-offs

- **Clipped wrapped lines inside a too-short row** (Decision 2's lower-bound approximation) → Mitigation: this is strictly no worse than today (today *every* row is at risk of this on a short terminal; after this change, only a row whose single line is wider than the panel and wraps is at risk, and only when that row is part of an overflowing screen). Not attempting full wrap-aware height measurement keeps the change scoped and avoids duplicating `Paragraph`'s own wrapping logic.
- **Existing tests that assert on fixed `Rect`s / `Constraint::Fill` layout** (`src/tui.rs` has unit tests around `draw`) → Mitigation: covered in tasks.md; the fits-on-screen path is byte-for-byte unchanged, so only overflow-specific tests need new assertions.
- **A resize mid-scroll could momentarily show a stale `scroll_offset`** before the next clamp runs → Mitigation: clamping happens every frame (right before `draw`), not only on scroll keys, so it self-corrects within one redraw (at most one `POLL_INTERVAL`, 100ms).
