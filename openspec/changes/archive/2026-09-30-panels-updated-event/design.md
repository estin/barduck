# Design

## Context

User scripts (`web_user_js`, change `add-webui-user-js`) load once per page in the static shell, outside the shard comment markers that topcoat's `morph` rewrites inside `#bd-panel-wrapper`. Panels re-render on each tick bump driven by the tick-span client script in `page_chrome` (`src/web/routes.rs:282-295`): SSE `refresh` events plus a `setTimeout` fallback. Per-source DOM anchors exist (`#panel-<source>`, `src/web/panels.rs:820,869`) and a per-tick status marker (`#bd-status[data-status]`, `panels.rs:774`). See proposal.md for motivation; see `specs/*/spec.md` for the contract.

## Goals / Non-Goals

**Goals:**
- One stable notification per panel refresh that user scripts can subscribe to once.
- Payload names rendered sources so listeners skip DOM discovery when uninterested.

**Non-Goals:**
- Per-source events (one event per refresh; filtering is the listener's job).
- Guaranteeing delivery to scripts that subscribe late (late subscribers miss earlier events — standard DOM semantics).
- A daemon-side (Rust) hook; the event exists only in the browser.

## Decisions

1. **Dispatch from the tick-bump client path, not from topcoat internals.**
   The `__bdRefresh.bump()` path in the tick-span script is the single funnel every refresh flows through (SSE + fallback). Emitting `document.dispatchEvent(new CustomEvent("barduck:panels-updated", …))` there avoids coupling to topcoat's `morph` completion callbacks. Timing note: dispatch on the tick bump (request) rather than after morph completes — shard re-render latency is one round-trip; listeners that need post-DOM state can `requestAnimationFrame` or re-query on the next tick. Alternative — MutationObserver guidance in docs only — rejected as the contract (fragile, per-script boilerplate), though still a valid user fallback.

2. **Source names in `detail`, read from the rendered page.**
   The shell knows the layout's source names at render time; the client script collects the currently rendered `#panel-<id>` anchors (or the server embeds the list). Exact mechanism is implementation detail; the contract is `detail.sources: string[]`. On log views the list is the viewed source.

3. **SKILL.md section placement: new "Custom Web UI Scripts" section after Layout Configuration.**
   Follows the existing doc shape (config table entry + short recipe + example). `web_user_js` also gets a row in the Top-Level Settings table. No binary-code change: `include_str!("../SKILL.md")` picks it up at build.

## Risks / Trade-offs

- [Risk] Event fires before morph completes, so a handler querying the DOM may read pre-refresh state → Mitigation: document `requestAnimationFrame`/deferred-query pattern in SKILL.md; payload already carries source names so most handlers need no DOM read.
- [Risk] Listeners throwing break other listeners → Mitigation: standard DOM semantics (one listener's throw doesn't stop dispatch to others when subscribed separately); document try/catch in the recipe.
- [Risk] Name collision with future daemon events → Mitigation: `barduck:` namespace prefix.

## Migration Plan

Additive: one DOM event + docs. Rollback = remove the dispatch line and the SKILL.md section.

## Open Questions

- None.
