# Changelog

All notable changes to this project are documented here.

## v0.1.7 (2026-10-12)

### Bug Fixes

- Require .toml config files and clean up the Loader hook

### CI

- Pin Rust 1.99.0 and satisfy its assert_is_empty lint

### Maintenance

- **nix:** Pin barduck-bin to v0.1.6
- **docs:** Add light mode screenshots

### Other

- Mirror real-world usage; regenerate both screenshots
- Move the config plane onto estin/bonfig

Load Config through figment layers over the built-in defaults: the named
TOML file, then a BARDUCK__ env layer. The hand-rolled
apply_env_overrides_from machinery is gone; env values now follow
figment's TOML-scalar rules (a string that looks like a number needs
quotes). Structural keys (sources, layouts) stay file-only: any env
value would replace the whole list.
- Route config loading through bonfig's Loader

Delegate the layering (defaults, named config file, BARDUCK__ env vars)
to Loader itself instead of hand-assembling figment providers, so
barduck tracks bonfig's layering semantics going forward. The extra
provider merge stays as the test hook.

## v0.1.6 (2026-10-07)

### Bug Fixes

- Anchor chips pinned to values
- **tests:** Scope group-panel color lookup to panel body lines
- Highlight is value only

### Maintenance

- **nix:** Pin barduck-bin to v0.1.5

## v0.1.5 (2026-10-06)

### Bug Fixes

- Don't lock db on read operations, ci fixes
- **tests:** Sleep 10s as workaround for overloaded ci runner
- Mid-fetch icon

### Maintenance

- **nix:** Pin barduck-bin to v0.1.4
- **docs:** Add screenshots

## v0.1.4 (2026-10-01)

### CI

- Allow install barduck as static binary via nix

## v0.1.3 (2026-10-01)

### Bug Fixes

- User script injection, ci pipelines

## v0.1.2 (2026-10-01)

### CI

- Release pipeline fixes

## v0.1.1 (2026-10-01)

### CI

- Allow rerun pipeline

## v0.1.0 (2026-10-01)

### Features

- Query and stream source types for all
- Ingest by http api
- Add cli skill command
- Allow to override the style in web ui for layout and cell
- Force poll
- Composite source
- Log pagination
- Support custom user script js
- Allow to change the content width for webui

### Bug Fixes

- Try to fix topcoat assets distribute
- Add favicon
- New layout config
- Dark/light, viewport for mobile devices, compact layout
- Force to reload all page on theme mod switch
- Render markdown for web ui, add option to show/hide source in tui/web modes
- Add retry_interval
- Pin status to the bottom of pane
- Use rectagles for history bar
- Constant, link to the root
- Query logic
- Remove stale_interval
- Fmt
- Db checkpoint
- Don't poll sources on startup if data is fresh
- Update theme contrast
- Offline indicator
- Typed values
- Update logs web ui, fix the startup check source schedule
- Add cli fetch command to debug source poll
- Remove health command, unify source param, add unit to the log views
- Allow render tables
- Override style, add new source ingest
- Sync and archive
- Review fixes
- Don't reopen connection
- Reuse writer connection
- Add cooldown
- Optimizations
- Update tui
- Sync
- Links to similar projects and SKILL.md note about ingest source type
- Promote
- The chips background color for non threshold source
- Tui scroll
- Platform agnostic
- Promote 2
- Logs webui page table field ordering
- Update topcoat
- Small refactoring
- Add project index
- Theme mode switch icon logic
- Sse
- Default config path
- Refresh on force poll
- Release command

### Refactoring

- Split modules, use one writer connection

### CI

- Initial
- Release tasks and build
- Fix release logic

### Maintenance

- Add link to homepage project
- Add link the vesta

### Other

- Initial commit: barduck
