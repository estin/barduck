# Design

## Context

See `proposal.md` — Why. The summary strip and panel grid are rendered by one topcoat shard (`panels_grid` in `src/web/panels.rs`) that re-renders on every refresh tick, so anything added to the DOM must survive a morph, not just the first paint. Chips are plain anchors and the page has no client-side router; there is no existing focus/highlight mechanism to reuse.

## Goals / Non-Goals

**Goals:**

- Make a chip click land on the specific source it names, and make the landing visible, without adding client-side JavaScript or state.
- Keep working when the panel grid re-renders underneath the user.
- Keep a markup-level contract that integration tests can assert (ids, hrefs, highlight rule).

**Non-Goals:**

- Reworking the summary strip's ordering, colors, or placement.
- Changing the log view (`/logs/<source>`) or the `/api` surface.
- Introducing smooth scrolling or a dismissable focus state.

## Decisions

**Per-placement value ids, scoped by the card's own anchor.** Every value element gets `id="value-<source>"`; when a source is not the card's first member (a `secondary`/`table` member, or a composite child), the id is `value-<card-anchor>-<source>`. The card id (`panel-<anchor>`) is shared by every member of a group, so it cannot address one member; a bare `value-<source>` would collide when a source is placed twice, e.g. a composite child that also has its own panel. Scoping by the already-unique card anchor gives one addressable value per rendered placement and keeps the standalone panel's id short.
- *Alternative considered:* a separate opaque counter per value — unique but not derivable from markup, so tests and debugging would need a lookup.

**Highlight via `:target` + CSS, not JavaScript.** The chip's `href` is the value's id, so the browser's own fragment navigation scrolls there and `:target` styling highlights it. Because `:target` is a function of the URL fragment and the element id, a shard re-render that keeps the id keeps the highlight — no observer, no click handler, nothing to re-apply.
- *Alternative considered:* a click handler that adds a `.bd-focused` class — needs a mutation observer to survive each refresh and duplicates what the URL already encodes.

**Card highlight as a separate `:has` rule.** `.bd-value:target` outlines the value; a second rule, `.bd-panel-cell:has(.bd-value:target)`, outlines the containing card. They are deliberately separate declarations: a browser without `:has` drops only its own rule and still highlights the value, rather than losing the whole block.
- *Alternative considered:* putting the target id on the card only — which is exactly the shared-card behavior this change replaces.

**`scroll-margin-top` on the value.** The header (and the offline banner) is a sticky block, so an unadjusted scroll would put the jumped-to value underneath it. The value reserves header height instead of adjusting scroll position in script.
- *Alternative considered:* `scroll-padding-top` on the scroll container — has to track the banner's show/hide, whereas `scroll-margin` follows the target automatically.

## Risks / Trade-offs

- **Duplicate ids if two cells resolve to the same anchor** (for example the same group rendered twice) → the pre-existing code already used `panel-<anchor>` as a card id with the same assumption; the value id inherits it rather than adding a new failure mode. Configs that render one card per source are unaffected.
- **A browser too old for `:has` loses the card outline** → the value itself still highlights, and the scroll still lands correctly; only the extra card emphasis degrades.
- **Clicking a chip whose hash is already the current one does not re-scroll** (no `hashchange`) → the value is already the focus target at that point, so there is nothing to change; no script is added to force a re-scroll.
- **Composed anchors the user script contract relies on** (`barduck:panels-updated` scans `[id^="panel-"]`) → value ids use the `value-` prefix and a `bd-value` class, so the existing scan and event payload are unaffected.

## Migration Plan

No data or config migration. The change is additive to markup and CSS; reverting the commit restores the previous `#panel-<anchor>` chip hrefs with no residual state.
