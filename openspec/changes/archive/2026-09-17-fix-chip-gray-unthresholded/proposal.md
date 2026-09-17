## Why

The source summary strip's chips currently default to **green** for a healthy source that declares no threshold bands, even though that source's own panel deliberately renders with no accent color at all for that same state (`accent_color` returns `None`; the panel is "plain neutral", not green — see the web-ui spec's "health visible at a glance" requirement). This misleads users into reading "green = actively confirmed good by a threshold" when really it just means "no threshold configured, currently healthy." It also violates the existing "Source summary strip" requirement that a chip's color must match the color its panel currently renders with.

## What Changes

- Chips for a source with no threshold bands that is currently healthy render **gray** (a new neutral chip style) instead of green.
- Chips for a source with no threshold bands that is currently `stale` render **yellow** (unchanged — already correct).
- Chips for a source with no threshold bands that is currently `failing` render **red** (unchanged — already correct).
- Chips for a source with threshold bands are unaffected: banded-healthy still shows the band's own color (green/yellow/red), and an active health problem still overrides the band color, exactly as today.
- The browser-tab favicon "worst status" indicator, which currently also treats unbanded-healthy as green, keeps its existing green fallback — it needs some color and green-for-healthy is the conventional favicon meaning; only the visible chip label changes.

## Capabilities

### New Capabilities
(none)

### Modified Capabilities
- `web-ui`: the "Source summary strip" requirement's chip-coloring rule changes for the unbanded-healthy case — a chip now has a neutral gray state distinct from green, matching what the panel itself renders.

## Impact

- `src/web/panels.rs`: `Panel::chip_style` (currently keyed off `level_color()`, which always resolves to a concrete `Level` and folds unbanded-healthy into green) must instead key off `accent_color()` (`Option<Level>`) and add a gray branch for `None`, mirroring how `status_style`/`group_row_style` already use `accent_color()`.
- `assets/styles.css` (or wherever chip tokens live): needs a neutral gray chip background/foreground pair alongside the existing `--status-{red,yellow,green}-*` tokens, following the same light/dark token pattern.
- No change to `worst_color`/favicon logic, no config schema change, no API change.
