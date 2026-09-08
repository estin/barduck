## Why

When the daemon becomes unreachable, today's only signal is the small connection dot/label near the page title flipping to a gray/red "offline" state (`src/web/routes.rs` `CONNECTION_SCRIPT`) — easy to miss, especially since it's the same size and position regardless of state. Separately, the browser-tab favicon (`FAVICON_SCRIPT`) only ever reflects the last known *health* status of sources (`bd-status`'s `data-status`, driven by the panel-refresh shard); it has no idea whether the connection itself is down, so during an outage it just freezes on whatever color it last had — it doesn't turn red for "I can't reach the server" the way it does for "a source is unhealthy." A user glancing at a browser tab, or at a dashboard they're not actively watching, has no reliable at-a-glance signal that the whole page has gone stale because the server is down.

## What Changes

- When the connection is offline, the favicon SHALL turn red, overriding whatever health-derived color it would otherwise show from `bd-status`. It reverts to reflecting health again as soon as the connection recovers.
- When the connection is offline, the dashboard SHALL additionally show a persistent, hard-to-miss banner (fixed at the top of the page, above the title) stating the connection is lost, and SHALL dim the main panel content to signal that what's on screen may be stale. Both clear automatically the instant the connection recovers — no page reload needed.
- The existing small connection dot/label stays as-is (still useful as a compact, always-present signal); this change adds the louder favicon + banner + dim treatment specifically for the offline case, since that's the state most likely to go unnoticed.
- No change to *how* offline is detected (still the existing `/api/ping` poll on the same 5-second interval) — only to what happens once it's detected.

## Capabilities

### Modified Capabilities
- `web-ui`: extends the "Global connection health indicator" requirement so an offline connection also drives the favicon color and a page-level dim + banner notice, not just the small dot/label.

## Impact

- `src/web/routes.rs`: `CONNECTION_SCRIPT` becomes the single source of truth for connection state (already owns the ping loop) and additionally toggles the banner/dim state and a shared flag the favicon script reads; `FAVICON_SCRIPT`'s `refresh()` checks that flag and overrides to red when offline; the dashboard's markup (`dashboard` page function) gains a hidden-by-default banner element and a dim-capable wrapper around the panel content.
- No backend/API changes — `/api/ping` behavior is unchanged; this is purely front-end reaction to its existing result.
- No changes to health-derived favicon coloring for the online case (unchanged from before this change).
