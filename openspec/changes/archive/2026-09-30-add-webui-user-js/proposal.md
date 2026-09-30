# Proposal

## Why

Users cannot customize the barduck web dashboard today — every tweak requires patching barduck's source. Allowing user-defined JavaScript to be injected into the web UI gives operators a supported extension point (extra widgets, integrations, behavior tweaks) without forking the daemon.

## What Changes

- New config option (`web_user_js`) accepting a list of paths; each path may be a concrete `.js` file or a directory of `.js` files.
- Directory entries expand to their `*.js` files in alphabetic (byte-wise filename) order, non-recursive; overall load order is config-list order, then alphabetic within each directory.
- The daemon injects a `<script>` tag per resolved file at the end of the web UI `<body>` (after the existing inline scripts), on both the dashboard and per-source log pages.
- The daemon serves the configured files over HTTP so the browser can load them (browsers cannot fetch `file://`), restricted to the startup-resolved file set.
- Injected tags live outside the topcoat shard render region, so shard refreshes (SSE tick / fallback timer) never re-create, remove, or re-execute them; scripts run once per page load.
- Missing/unreadable configured paths produce a warning and are skipped; they do not prevent daemon startup.

## Capabilities

### New Capabilities

_(none)_

### Modified Capabilities

- `web-ui`: script injection at end of `<body>`; HTTP serving of user scripts; guarantee that injected `<script>` tags survive topcoat shard updates without re-execution.
- `source-configuration`: new `web_user_js` config option (list of file/dir paths, alphabetic directory expansion, defaults, validation/error handling).

## Impact

- `src/web/routes.rs` — `page_chrome` emits injected tags after the existing end-of-body scripts.
- `src/api.rs` / `src/lib.rs` — new route serving user JS files.
- `src/config/mod.rs`, `src/config/defaults.rs` — new option, default, validation.
- No breaking change: option defaults to empty; existing configs keep working (`deny_unknown_fields` only rejects unknown keys, and this adds a known one).
- Security surface: daemon gains a file-serving path constrained to the startup-resolved user-JS set.
