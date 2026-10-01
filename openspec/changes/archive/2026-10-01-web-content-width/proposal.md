# Proposal

## Why

The dashboard shell currently renders full-width for every viewer: a wide
monitor gets edge-to-edge panels, but a viewer who prefers a centered narrow
column has no way to get one. Content width is a per-viewer preference, not a
per-deployment fact, so it needs a per-browser setting with a server-wide
default — the same split the existing light/dark theme toggle already uses.

## What Changes

- New optional top-level config key `web_content_width`: `"narrow"`
  (centered capped column, today's default) or `"wide"` (full viewport
  width). Default `"narrow"`.
- New per-browser override persisted in a `bd_width` cookie, read by the
  server on every page load so the first byte already carries the right
  width (no flash of the wrong layout).
- New `POST /api/width` endpoint accepting `"wide"`/`"narrow"`, mirroring
  `POST /api/theme` (400 on anything else, 1-year cookie).
- New width toggle button in the header next to the theme toggle, posting
  the newly selected width and updating the page without a reload.
- `SKILL.md` documents the new key alongside the other web settings.

## Capabilities

### New Capabilities

None — the behavior extends the existing web UI surface.

### Modified Capabilities

- `web-ui`: content width becomes a server default plus per-browser
  override; header gains a width toggle; new `POST /api/width` endpoint.
- `source-configuration`: new optional `web_content_width` key with
  validation (only the two tokens accepted).
- `barduck-skill-doc`: `SKILL.md` gains the new key.
