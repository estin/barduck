# Proposal

## Why

The summary strip's chips currently all link to the card a pane's members share, so several chips — a generalized pane's `secondary`/`table` members, or a composite root's children — jump to the same place and no chip identifies the value the user actually asked about. The spec requires a click to "navigate to and visually highlight that source's panel", but with the anchor on the shared card that behavior is indistinguishable between members and gives no visible focus feedback at all.

## What Changes

- Give every rendered value a stable DOM id (`value-<source>`, scoped by the card's first member when a source appears in more than one cell) and a shared `bd-value` class.
- Point each summary-strip chip at its own source's value id instead of the shared `#panel-<anchor>` card.
- On a chip click, scroll the source's value into view — clearing the sticky header — and outline it; extend the outline to the card containing it.
- Keep the highlight applied across the live panel re-renders, since the anchor is the URL fragment and the ids survive each refresh.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-ui`: The **Source summary strip** requirement's click behavior is sharpened. Clicking a chip must navigate to that source's *own* value and highlight it (and the card it sits in), rather than only the card shared by a pane's members; a source placed in more than one cell must get a distinct, addressable value element per placement.

## Impact

- `src/web/panels.rs`: chip `href`s and the value elements' ids/classes.
- `src/web/routes.rs`: the page's inline `:target`/`:has` highlight CSS.
- `tests/integration.rs`: chip-anchor and highlight coverage.
- No config, API, storage, or dependency changes.
