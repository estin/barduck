## Context

All theming lives in `assets/styles.css` as CSS custom properties: base tokens (`--background`, `--foreground`, `--muted-foreground`, `--primary`, `--destructive`, `--border`, `--ring`, shadows) as `oklch(...)`, and barduck's own `--status-{red,yellow,green}-{border,bg,fg,text,chip-fg}` tokens as hex (kept as hex per the file's own comment, "to match the values this project already shipped before theming existed"). Light values live on `:root`; dark values are duplicated in two places that must stay in sync (`@media (prefers-color-scheme: dark) { :root:not(.light) {...} }` for OS-preference-only visitors, and `.dark {...}` for an explicit stored preference) — both blocks currently carry identical values and both need the same edit.

Two rendering paths bypass these tokens entirely today: `src/web/panels.rs`'s `segment_class` (history bar) uses raw Tailwind classes (`bg-red-500`, `bg-amber-400`, `bg-emerald-500`, `bg-slate-200`) with no `dark:` variant, and `src/web/routes.rs`'s connection-indicator/favicon inline `<script>` blocks hardcode hex (`#94a3b8`/`#10b981`/`#ef4444`, `#767d78`). See proposal.md - Why for how the token gaps were found.

## Goals / Non-Goals

**Goals:**
- Fix the specific, identified root cause: dark mode's `--status-green-border`, `--status-yellow-border`, `--status-red-chip-fg`, `--status-yellow-chip-fg`, `--status-green-chip-fg` have no override and fall through to light mode's saturated values; and `--status-yellow-text`'s existing dark override is more saturated than its light value.
- Moderate the dark theme's base background/foreground contrast (less near-black, less stark-white) without sacrificing legibility.
- Bring the two token-unaware rendering paths (history bar, connection indicator/favicon) onto the same tokens so they get dark treatment too.

**Non-Goals:**
- Changing which level (red/yellow/green) a source's health or threshold band resolves to — only what color each level renders as.
- A from-scratch redesign of the color system or component structure.
- Pixel-matching the Dark Reader extension's actual color-remapping algorithm — it's used here only as a reference point for "muted, not neon," not as a spec to replicate exactly.
- Changing light-theme token values (see Decisions below — reviewed, not changed).

## Decisions

- **Root-cause-first, not palette-from-scratch**: the loudest visual problem (neon green/orange summary chips, glowing yellow load value) traces to five *missing* dark overrides plus one *inverted* one (dark yellow-text brighter than light's), not to the base background/foreground tokens being fundamentally wrong. Fixing those first is the highest-leverage, lowest-risk change; the base-token softening below is a secondary, smaller adjustment layered on top, per the user's explicit choice to do a full palette pass rather than a token-gap fix alone.
- **Status tokens stay hex, not oklch**: preserves the existing file's stated convention and keeps the diff minimal/reviewable color-by-color.
- **Chip text pattern made consistent**: today, yellow chips already use dark text on a light-ish fill (`chip-fg: #451a03` on `border: #fbbf24`); green/red chips (once given real dark values) adopt the same "dark text on a medium-toned fill" pattern instead of white text on a saturated fill, for a consistent, less glowing chip appearance across all three levels in dark mode.
- **Light theme reviewed, left unchanged**: light's background (`oklch(0.995 0.003 260)`) and foreground (`oklch(0.24 0.012 260)`) already avoid pure white/black, and its status hex values (`#ef4444`/`#fbbf24`/`#10b981` borders with darker `-fg` text) are conventional, moderate-contrast web colors on a light surface — the "neon on near-black" effect that motivated this change doesn't occur on a light background. No requirement or token change for light; this satisfies the review without manufacturing unnecessary risk to a theme nobody reported a problem with.
- **History bar and connection indicator move to tokens, not just new hardcoded dark variants**: adding a `dark:bg-*` Tailwind variant per segment would fix the immediate symptom but leave a second hardcoded palette to keep in sync forever. Routing them through the same `--status-*` tokens (or a `dark:` variant that reads the same underlying color intent) means future palette tuning only ever happens in `assets/styles.css`.

### Proposed token values

Dark-mode base tokens (`@media (prefers-color-scheme: dark) { :root:not(.light) }` and `.dark`, kept identical in both, per existing file convention):

| Token | Current | Proposed | Why |
|---|---|---|---|
| `--background` | `oklch(0.17 0.012 260)` | `oklch(0.20 0.014 260)` | less near-black |
| `--foreground` | `oklch(0.93 0.006 260)` | `oklch(0.88 0.008 260)` | less stark-white |
| `--muted-foreground` | `oklch(0.68 0.012 260)` | `oklch(0.64 0.012 260)` | keep proportional gap to the new background |
| `--primary` | `oklch(0.92 0.008 260)` | `oklch(0.85 0.01 260)` | less bright emphasis color |
| `--primary-foreground` | `oklch(0.21 0.015 260)` | `oklch(0.22 0.015 260)` | unchanged in practice |
| `--destructive` | `oklch(0.66 0.18 25)` | `oklch(0.62 0.13 25)` | lower chroma, consistent with status-red softening |
| `--destructive-foreground` | `oklch(0.14 0.02 25)` | `oklch(0.16 0.02 25)` | unchanged in practice |
| `--border` | `oklch(0.3 0.012 260)` | `oklch(0.32 0.014 260)` | small bump to track the lighter background |
| `--ring` | `oklch(0.6 0.05 260)` | `oklch(0.58 0.045 260)` | minor desaturation |
| `--shadow-xs`/`--shadow-sm` opacity | `35%`/`30%` | `42%`/`36%` | preserve card separation now that the background is lighter |

Dark-mode status tokens (hex; blank "Current" = presently unoverridden, falls through to the light value shown):

| Token | Current (dark) | Proposed (dark) |
|---|---|---|
| `--status-red-border` | `#f87171` | `#f87171` (unchanged — not implicated) |
| `--status-red-bg` | `#3f1315` | `#3f1315` (unchanged) |
| `--status-red-fg` | `#fecaca` | `#fecaca` (unchanged) |
| `--status-red-text` | `#f87171` | `#f87171` (unchanged) |
| `--status-red-chip-fg` | *(falls through to `#ffffff`)* | `#3f1013` — dark text on the medium-light red chip fill, matching the yellow chip's existing pattern |
| `--status-yellow-border` | *(falls through to `#fbbf24`)* | `#c9944e` — muted amber, the chip fill and panel border color |
| `--status-yellow-bg` | `#3a2a0d` | `#3a2a0d` (unchanged) |
| `--status-yellow-fg` | `#fde68a` | `#fde68a` (unchanged) |
| `--status-yellow-text` | `#fbbf24` **(brighter than light's `#d97706`)** | `#dda75d` — muted amber, no brighter than light's own yellow-text |
| `--status-yellow-chip-fg` | *(falls through to `#451a03`)* | `#451a03` (kept — already dark text on a light-ish fill) |
| `--status-green-border` | *(falls through to `#10b981`)* | `#4f9e7c` — muted sage-green, the chip fill and panel border color |
| `--status-green-bg` | `var(--background)` | unchanged (already swaps correctly) |
| `--status-green-fg` | `var(--foreground)` | unchanged (already swaps correctly) |
| `--status-green-text` | `#34d399` | `#5cbf94` — muted, still legible as plain text |
| `--status-green-chip-fg` | *(falls through to `#ffffff`)* | `#0d2e21` — dark text on the medium-toned green chip fill |

Exact final values are refined visually during implementation (task list includes a manual side-by-side comparison against `/tmp/darkreader.png`/`/tmp/current.png`); the table above is the starting point, not a pixel-locked spec.

### History bar and connection indicator

- `src/web/panels.rs` `segment_class`: replace `bg-red-500`/`bg-amber-400`/`bg-emerald-500`/`bg-slate-200` with inline `style="background-color:var(--status-{level}-border)"` (or equivalent), matching the pattern `chip_style`/`accent_style` already use elsewhere in the same file, so segments pick up the same values as chips and panel borders. The neutral (non-numeric) segment keeps a `--border`/`--muted-foreground`-derived neutral instead of the hardcoded `bg-slate-200`.
- `src/web/routes.rs` connection-indicator (`CONNECTION_SCRIPT`) and favicon (`FAVICON_SCRIPT`) inline JS: read the same colors via `getComputedStyle(document.documentElement).getPropertyValue('--status-...')`/`--foreground` etc. at the point they're applied, instead of hardcoded hex, so they track whichever theme is active without duplicating the palette in JS.

## Risks / Trade-offs

- [Manually re-picking hex/oklch values risks landing somewhere that still looks "off" without a live visual check] → Mitigated by a dedicated manual-comparison task in tasks.md against the two reference screenshots, plus checking both the OS-preference dark path and the explicit-cookie `.dark` path (they must stay in sync, per the file's existing comment).
- [`getComputedStyle` lookups in the connection/favicon scripts add a small runtime cost on every 5s poll] → Negligible: a handful of CSS custom-property reads, no layout thrust; same polling cadence as today.
- [Softening `--destructive` affects any use of that token outside status coloring (e.g. form/button destructive states), which wasn't part of the reported complaint] → Low risk: it's barely used in the current UI (no destructive buttons/forms visible in the dashboard today), and softening it is directionally consistent with the rest of this change.

## Migration Plan

Pure CSS/JS value changes plus two small Rust rendering-path edits (history bar, connection/favicon scripts); no data migration, no config changes. Ships as a normal code change. Rollback is reverting the commit.
