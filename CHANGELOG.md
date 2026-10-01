# Changelog

All notable changes to this project are documented here.

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
