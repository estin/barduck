# Tasks

## 1. Per-source value anchors

- [x] 1.1 Add `Slot::value_id(source)` in `src/web/panels.rs`, returning `value-<source>` and scoping to `value-<card-anchor>-<source>` when the source is not the card's first member; verify the composite integration test sees distinct `value-load::1m` and `value-load::1m-load::5m` ids for the two placements of `load::5m`
- [x] 1.2 Give every rendered value an id from `value_id` and the shared `bd-value` class — the single-source panel's content wrapper, and the group/composite `main`, `secondary`, and `table` values; verify the page HTML carries `id="value-balance"` / `id="value-days-left-<member>"`
- [x] 1.3 Point each summary-strip chip's `href` at its own `value_id` instead of the shared `#panel-<anchor>` card; verify `web_ui_summary_chip_href_matches_value_id` passes and `web_ui_summary_strip_lists_chips_in_layout_order_with_matching_colors` still finds the chips in layout order

## 2. Focus highlight

- [x] 2.1 Add `.bd-value:target { outline: … }` and the separate `.bd-panel-cell:has(.bd-value:target)` rule to the page's inline style in `src/web/routes.rs`; verify the served page contains both rules and a browser reports a solid 2px outline on the targeted value and its card
- [x] 2.2 Add `scroll-margin-top` to the target so the pinned header does not cover it; verify a browser probe reports the jumped-to value's top below the header and inside the viewport
- [x] 2.3 Write the chip-anchor and highlight documentation into the capability spec delta and record the design decisions (id scheme, CSS `:target` over script, `:has` fallback) in `design.md`; verify `openspec validate anchor-chips-to-source-value` passes

## 3. Behavior coverage

- [x] 3.1 Add `web_ui_chip_links_each_group_member_to_its_own_value`: each chip href resolves to its own member's value element id and the group card keeps its `panel-` anchor; verify it passes
- [x] 3.2 Extend `web_ui_composite_root_renders_as_a_table_of_its_children` to assert the per-placement value ids and chip hrefs for a composite child that also has a standalone panel; verify it passes
- [x] 3.3 Update the chip tests that assumed `#panel-<source>` hrefs (`web_ui_summary_chip_href_matches_value_id`, the hidden-source test, the summary-order test) so the suite reflects the new contract; verify the full integration suite passes

## 4. Integration verification

- [x] 4.1 Run `cargo fmt --all -- --check` and `cargo clippy --locked --all-targets -- -D warnings`; verify both are clean
- [x] 4.2 Confirm in a headless browser against the demo daemon that clicking a chip scrolls to the named value, outlines it and its card, and that the highlight survives a live panel re-render; verify the probe reports the target id, a solid outline, the value in view, and the same state after the refresh interval
