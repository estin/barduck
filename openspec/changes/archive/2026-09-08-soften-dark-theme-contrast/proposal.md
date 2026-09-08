## Why

The dark theme reads as harsh and "neon" against its near-black background (user's own words: "too contrast" and "poison colors"), compared to the reference look produced by the Dark Reader browser extension over the same page (`/tmp/darkreader.png` vs `/tmp/current.png`). Tracing the actual CSS (`assets/styles.css`) shows this isn't a vague aesthetic gap: the dark theme (`@media (prefers-color-scheme: dark)` / `.dark`) never overrides `--status-green-border`, `--status-yellow-border`, `--status-red-chip-fg`, `--status-yellow-chip-fg`, or `--status-green-chip-fg` — those five tokens silently fall through to the light theme's fully-saturated values (`#10b981` green, `#fbbf24` yellow, `#ffffff`/`#451a03` chip text), so every summary-strip chip and every green-bordered panel renders at full light-mode saturation against a near-black page. Separately, `--status-yellow-text` (the color of a banded value like "8.46 load") is *already* overridden for dark mode, but to `#fbbf24` — more saturated than light mode's own `#d97706` — which is backwards and is exactly the glowing-yellow number visible in the screenshot.

## What Changes

- Add the missing dark-mode overrides for `--status-green-border`, `--status-yellow-border`, `--status-red-chip-fg`, `--status-yellow-chip-fg`, and `--status-green-chip-fg`, using desaturated values calibrated for a dark background instead of falling through to light-mode's saturated hex values.
- Re-tune `--status-yellow-text` (and lightly re-check the other already-overridden dark status tokens) so no status color in dark mode is more saturated/brighter than its light-mode counterpart.
- Moderately soften the dark theme's base tokens (`--background`, `--foreground`, `--muted-foreground`, `--primary`, `--destructive`, `--border`) so the background is a dark gray rather than reading as near-black and body text is off-white rather than stark white — narrowing, not eliminating, the light/dark contrast gap.
- Tokenize the two remaining hardcoded, theme-unaware color spots so they inherit the same dark treatment instead of staying fixed at light-mode saturation regardless of theme: the panel history bar's segment classes (`bg-red-500`/`bg-amber-400`/`bg-emerald-500` in `src/web/panels.rs`) and the connection-indicator/favicon inline-JS hex colors (`src/web/routes.rs`). This also closes a pre-existing gap against the "Consistent token-based visual theme" requirement, which already calls for the connection indicator and history bar to be token-driven.
- Review the light theme for the same "avoid near-black/near-white extremes" principle. Conclusion recorded in design.md: light's existing tokens (`oklch(0.995...)` background, `oklch(0.24...)` foreground, moderate-saturation status hex) already avoid pure white/black and aren't reported as a problem, so no light-theme token values change.
- No new fields, no config changes, no changes to which color (red/yellow/green) a source's health or threshold band resolves to — only the concrete color values those levels render as in dark mode.

## Capabilities

### Modified Capabilities
- `web-ui`: adds a requirement that the dark theme keep its background/foreground contrast moderate and its status colors desaturated relative to light mode, instead of the light theme's saturated values leaking through unadapted.

## Impact

- `assets/styles.css`: dark-mode token block(s) (`@media (prefers-color-scheme: dark) { :root:not(.light) {...} }` and `.dark {...}`, which must stay in sync per the file's own existing comment) gain the missing `--status-*` overrides and revised base-token values.
- `src/web/panels.rs`: `segment_class` (history bar) changes from raw Tailwind color classes to token-based ones (or theme-aware equivalents).
- `src/web/routes.rs`: the connection-indicator and favicon inline-JS color maps read from the same tokens instead of hardcoded hex, so they track the theme.
- No changes to `src/config/*` (threshold band / health-level logic is unaffected — only the token values those levels render as).
- Purely visual: no new tests beyond confirming the right CSS variables exist and resolve correctly; no behavior change to what triggers red/yellow/green.
