# Proposal

## Why

Two defects in the user-script feature (`add-webui-user-js`, `panels-updated-event`):

1. **Event fires before the re-render.** `barduck:panels-updated` is dispatched from the tick `bump()`, which only *requests* a shard re-render; the new HTML arrives after a network round trip and is morphed in later. A handler that edits panel DOM — even deferred a frame, as SKILL.md advised — edits the old DOM, and the morph then reverts it. Real-world symptom: a `links-new-tab.js` script setting `target="_blank"` on panel links has no lasting effect. The spec already says the event fires *after* the re-render; the implementation does not.
2. **Served names are opaque.** Scripts are served as `/assets/user-js/u0.js`, `u1.js`, …, so browser devtools, error stack traces, and saved pages show `u0.js` instead of the user's file name, making the injected scripts hard to identify and debug.

## What Changes

- `barduck:panels-updated` is dispatched only once the re-rendered shard content is in the DOM: each shard render stamps its hidden `#bd-status` marker with the render's tick (`data-tick`), and a static page script observes the panel wrapper and dispatches when that stamp changes. Handlers can read and modify the fresh panel DOM directly, without deferring. DOM changes made by handlers never re-trigger the event.
- Each injected user script is served under its own file name (`/assets/user-js/links-new-tab.js`). Characters outside `[A-Za-z0-9._-]` are replaced with `_`; when two resolved files share a name, later ones get a numeric suffix before the extension (`foo-2.js`) and a startup warning names both paths.
- SKILL.md: drop the "defer a frame" advice; document the served names.

## Capabilities

### Modified Capabilities

- `web-ui`: panels-updated event timing (after the DOM update); user-script URLs use the file's own name.
- `barduck-skill-doc`: SKILL.md recipe no longer defers DOM reads.

## Impact

- `src/web/routes.rs`, `src/web/panels.rs` — marker stamp, dispatch script, tick `bump()` no longer dispatches.
- `src/config/mod.rs` — served-name derivation.
- `SKILL.md`, tests.
- Not breaking for scripts: the event name and `detail` shape are unchanged; only its timing moves later. Script URLs change, but they were never part of the config contract.
