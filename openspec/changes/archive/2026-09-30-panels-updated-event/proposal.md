# Proposal

## Why

User scripts injected via `web_user_js` (change `add-webui-user-js`) run once per page load in the static shell, while panels re-render on every shard tick. Without a notification hook, a user script that customizes panels per source must poll the DOM or wire a `MutationObserver` itself — fragile and wasteful. A daemon-dispatched custom event per panel refresh gives user scripts a stable extension point.

## What Changes

- After every panel-grid shard re-render, the page dispatches a `barduck:panels-updated` `CustomEvent` carrying the rendered source names, so user scripts subscribed once via `addEventListener` observe every refresh without re-subscribing.
- The event fires on both the dashboard and per-source log views, on stream-driven and fallback-timer refreshes alike.
- `SKILL.md` gains a "Custom Web UI Scripts" section documenting `web_user_js` (file/dir entries, alphabetic order, config-dir-relative) and the `barduck:panels-updated` listener recipe with a short example. `SKILL.md` is embedded in the binary via `include_str!`, so the doc change ships with the next build.

## Capabilities

### New Capabilities

_(none)_

### Modified Capabilities

- `web-ui`: `barduck:panels-updated` event dispatched after each panel refresh, with rendered source names in the payload.
- `barduck-skill-doc`: SKILL.md documents `web_user_js` configuration and the panels-updated listener recipe.

## Impact

- `src/web/routes.rs` — tick-span client script dispatches the event on each tick-driven refresh.
- `SKILL.md` — new section (also embedded in the binary; no code change needed beyond the doc edit).
- No breaking change: additive event + docs; existing pages gain one DOM event listeners can ignore.
