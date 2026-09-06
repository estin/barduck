## Context

See proposal.md - Why. Layouts are declared once in config and shared verbatim between the TUI and web renderers (`spec: source-configuration — UI layouts are config-declared like sources`). Both renderers currently build their grid straight from the same parsed layout + resolved source data, with no per-view filtering step. `show_in` needs to be resolved at render time, per view, without touching the shared layout-parsing/validation code that both UIs currently call identically.

## Goals / Non-Goals

**Goals:**
- Add a `show_in` field to a source's config (`"all" | "tui" | "web"`, default `"all"`).
- Give each renderer (TUI, web) a shared way to decide, per cell/member, whether a source is visible in *this* view.
- Keep layout parsing, column-count computation, and unknown-source validation completely unchanged — `show_in` never affects whether a config is valid, only what gets drawn.

**Non-Goals:**
- No global on/off switch that stops collection (that's out of scope per proposal.md).
- No per-layout or per-cell visibility override — visibility is a property of the source, not the cell referencing it.
- No change to static-text panel cells (they have no source).

## Decisions

**`show_in` lives on the source, not the cell.** A source's visibility is a single fact, and the same source can appear in multiple cells/panes across multiple layouts; declaring it once on the source avoids repeating it at every reference and avoids the two references disagreeing about the same source. Alternative considered: a per-cell override — rejected because the user's request is about the source's identity ("hide *this source* from the web dashboard"), not about one particular placement.

**Resolution happens where each renderer already resolves a cell's source(s) to render data, not in the shared layout-parsing step.** Layout parsing/validation stays view-agnostic (a layout is valid or not, independent of which UI renders it); each renderer, when it walks the parsed layout to build its own grid, checks `show_in` against its own view name and substitutes a space (or omits a pane member) instead of the normal render. Alternative considered: filtering at parse time into two separate view-specific layouts — rejected, since it would duplicate the row/column-count logic (currently computed once, shared by both UIs) and risk the two views disagreeing on geometry.

**A generalized pane with every member hidden collapses to a space cell**, reusing the exact "empty cell of this span" rendering both UIs already have for explicit `space` cells, rather than inventing a second "empty pane" visual. This keeps the space-rendering path single-implementation per UI.

## Risks / Trade-offs

- [A source visible only in one view can silently disappear from the other if `show_in` is set by mistake, with no startup error] → Acceptable per the clarified requirements (this is a display preference, not a config error); the "Default is visible everywhere" scenario keeps the safe default so an omitted field never surprises anyone.
- [Per-pane-member hiding adds a bit of branching to the already-multi-section generalized-pane renderer in both UIs] → Contained to the same place each renderer already loops over `main`/`secondary`/`table` members to render them; no new data flow.

## Migration Plan

Additive, backward-compatible: existing configs have no `show_in` field on any source, which defaults to `"all"` — identical behavior to before this change. No migration steps needed.
