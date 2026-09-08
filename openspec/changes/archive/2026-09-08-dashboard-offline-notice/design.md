## Context

`src/web/routes.rs` already has two independent, always-running polling scripts embedded as plain JS string constants: `CONNECTION_SCRIPT` (pings `/api/ping` every 5s, drives the small dot/label) and `FAVICON_SCRIPT` (reads the hidden `#bd-status[data-status]` marker `panels_grid` renders, also every 5s, and paints the favicon red/yellow/green accordingly). They currently share nothing — `FAVICON_SCRIPT` has no idea whether the connection is up. See proposal.md - Why.

## Goals / Non-Goals

**Goals:**
- Make "the connection is offline" produce a red favicon, a persistent banner, and a dimmed panel area — not just the small dot/label.
- Keep the existing offline-detection mechanism (the `/api/ping` poll) exactly as-is; only react differently to its result.

**Non-Goals:**
- A toast/notification-stack component. The user's own request left the exact mechanism open ("toast or dim the container or something else"); a fixed banner + dim is simpler, needs no new component or dependency, and (unlike a toast that auto-dismisses) naturally stays visible for exactly as long as the condition persists, which matches "the server is currently unreachable" better than a transient toast would.
- Changing favicon coloring for the *online* case, or anything about how health status itself is computed.

## Decisions

- **`CONNECTION_SCRIPT` becomes the single source of truth for connection state**, since it already owns the ping loop. Its `setState()` writes the current state to `document.body.dataset.bdConnection` in addition to updating the dot/label; `FAVICON_SCRIPT`'s `refresh()` (which already runs its own independent 5s interval) reads that attribute before falling back to the health-derived color. This avoids a second, duplicate ping loop in the favicon script and avoids inventing a custom event/pub-sub mechanism for two scripts that already share the same document — a DOM attribute is the simplest shared channel available. Alternative considered: a `CustomEvent` dispatched on state change — rejected as unnecessary indirection when `FAVICON_SCRIPT` already polls on its own timer and can just read the attribute at that point.
- **Banner + dim toggled from the same `setState()`**, for the same reason (one state transition, one place that reacts to it). No new polling loop.
- **Banner and dim styled via the existing `--status-red-*` tokens** (`border`/`bg`/`fg`), not raw Tailwind color utilities, to stay consistent with the "Consistent token-based visual theme" requirement and the recently-completed dark-theme softening — the banner should look like the same muted red used everywhere else in dark mode, not a separate hardcoded red.
- **Dim implemented as an opacity + pointer-events toggle on the existing panel-grid wrapper `<div>`** (Tailwind's own `opacity-50 pointer-events-none`, toggled via `classList`), not a new overlay element — simplest option, no new DOM structure beyond the banner itself, and disabling pointer events during an outage avoids the confusing appearance of clickable-but-nonfunctional (stale) panel links.
- **Banner is a single fixed-position element, hidden by default via the standard `hidden` attribute**, shown/hidden by `setState()` — matches how the connection dot/label and favicon already work (server-rendered hidden/neutral, then driven entirely by client JS), so there's no new pattern to reason about.

## Risks / Trade-offs

- [A fixed-position top banner shifts page content or overlaps the existing header when it appears/disappears] → Mitigate by giving the banner a reserved height that pushes content down via a flow-layout placement (inserted before the page's own header content, not `position: fixed`), rather than overlaying — avoids layout jump *and* overlap. (Revisit at implementation time if a fixed overlay turns out to read better; either way it must not permanently reserve space while online.)
- [Toggling dim/pointer-events on every 5s tick even when the state hasn't changed] → `setState()` already only runs when `ping()`'s `.then`/`.catch` resolves, i.e. once per interval tick regardless of state change; toggling idempotent classes/attributes on every tick is harmless (no visible flicker from re-applying the same class).
- [Dimming panel content during an outage might itself be mistaken for a rendering bug by a user unfamiliar with the feature] → Mitigated by pairing it with the explicit banner text, so the dim is always accompanied by an explanation.

## Migration Plan

Pure front-end change (JS string constants + a small markup addition in `src/web/routes.rs`); no data, config, or API changes. Ships as a normal code change; rollback is reverting the commit.
