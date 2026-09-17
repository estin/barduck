## 1. Row natural-height computation

- [x] 1.1 Add a function computing one `Slot`'s content-line count (static-text: `text.lines().count()`; single-panel: `value.lines().count()`; group pane: `main.is_some() as usize + secondary.len() + table.len()`; spacer: 0), and a per-row natural height (`2 + row.iter().map(content_lines).max().unwrap_or(0)`, floored at 3), in `src/tui.rs`. Verify with unit tests covering each slot kind and the floor case (an all-spacer row).
- [x] 1.2 Add a function computing all rows' natural heights plus their total, given `&[&Vec<Slot>]`. Verify with a unit test on a small fixture of rows with mixed slot kinds.

## 2. Scroll state and clamping

- [x] 2.1 Add `scroll_offset: usize` to `UiState`, initialized to `0`. Verify it compiles and `event_loop`'s existing construction of `UiState` is updated.
- [x] 2.2 Add a function computing `max_offset` for given row natural heights and available body height (walk backward from the last row accumulating heights until one more would exceed the available height). Verify with unit tests: content shorter than the screen (`max_offset == 0`), content exactly filling the screen (`max_offset == 0`), and content taller than the screen (`max_offset` matches hand-computed expectation for a small fixture).
- [x] 2.3 Call the clamp (`scroll_offset = scroll_offset.min(max_offset)`) once per frame in `event_loop`, right before `draw()` is invoked, using the current terminal size and the freshly computed row heights. Verify with a unit test (or by inspection of `event_loop`) that a shrinking terminal or fewer rows after a refresh cannot leave `scroll_offset` past `max_offset`.

## 3. Two-mode row layout in `draw()`

- [x] 3.1 In `draw()`, compute `total_natural` and, when `total_natural <= body.height`, keep today's exact code path (`Layout::vertical(vec![Constraint::Fill(1); rows.len()])` over all rows). Verify `draw_renders_panels_and_error_banner` and other existing `draw`-related tests still pass unchanged.
- [x] 3.2 When `total_natural > body.height`, slice `rows[state.scroll_offset..]`, take rows while their cumulative natural height fits `body.height`, and lay out only that slice with `Layout::vertical` using `Constraint::Length(natural_height[i])` per visible row (leaving any leftover space at the bottom blank). Verify with a new test: a `TestBackend` sized shorter than N rows' total natural height renders only the rows that fit, starting from `scroll_offset`.

## 4. Keybindings

- [x] 4.1 In `event_loop`'s key-handling branch (next to `quit_requested`), add scroll key handling: `↑`/`k` decrements `scroll_offset` (saturating at 0), `↓`/`j` increments then clamps to `max_offset`, `PgUp`/`PgDn` move by the current viewport's row count, `Home`/`g` sets `0`, `End`/`G` sets `max_offset`. Verify with unit tests for each key (mirroring `quit_keys_are_recognized`'s style) confirming the resulting `scroll_offset` for a fixture with known `max_offset`.
- [x] 4.2 Verify scroll keys are effectively no-ops when `max_offset == 0` (content fits) — a unit test asserting `scroll_offset` stays `0` after any scroll key when rows fit the screen.

## 5. Scroll indicator

- [x] 5.1 Render a `ratatui::widgets::Scrollbar` on the content area's right edge, built from `ScrollbarState::new(rows.len()).position(state.scroll_offset)`, only when `total_natural > body.height`. Verify with a test asserting the scrollbar's characters appear in the rendered buffer when content overflows and do not appear when it fits.

## 6. Verification

- [x] 6.1 Run `just ci` (clippy + nextest) and verify it passes.
- [x] 6.2 Manually run `just demo` in a deliberately short terminal (or resize the terminal below the demo layouts' combined height) and verify: all rows are reachable via `↑`/`↓`/`j`/`k`/`PgUp`/`PgDn`/`Home`/`End`/`g`/`G`, the scrollbar appears only while scrolled content exists, and resizing the terminal back to full height restores the original unscrolled, evenly-stretched layout with no scrollbar.
