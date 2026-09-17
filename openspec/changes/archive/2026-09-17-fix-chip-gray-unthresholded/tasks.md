## 1. Neutral chip styling

- [x] 1.1 Add a neutral gray chip token pair to `assets/styles.css` (light and dark theme blocks), following the existing `--status-{red,yellow,green}-border` / `--status-{red,yellow,green}-chip-fg` naming pattern, and verify the build's CSS bundle includes the new variables in both theme blocks.

## 2. Chip coloring logic

- [x] 2.1 Change `Panel::chip_style` in `src/web/panels.rs` to branch on `self.accent_color()` (`Option<Level>`) instead of `self.level_color()`, adding a `None` arm that returns the new gray chip style, and verify the existing `Red`/`Yellow`/`Green` arms are unchanged in output.
- [x] 2.2 Confirm `Panel::level_color()` (used for `worst_color`/the favicon marker) is left untouched, so the browser-tab favicon still falls back to green for unbanded-healthy — verify by re-reading the diff and confirming `chips_grid`'s `worst` computation still calls `level_color()`, not `accent_color()`.

## 3. Verification

- [x] 3.1 Run `just ci` (clippy + nextest) and verify it passes.
- [x] 3.2 Manually run the demo (`just demo`) and verify in the browser: a source with no threshold bands shows a gray chip while healthy, a yellow chip while stale, and a red chip while failing; a threshold-banded source's chip is unaffected.
