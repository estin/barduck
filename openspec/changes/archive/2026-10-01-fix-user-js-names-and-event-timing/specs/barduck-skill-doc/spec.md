# Spec Delta

## MODIFIED Requirements

### Requirement: SKILL.md documents custom web UI scripts
SKILL.md SHALL document the `web_user_js` option — a top-level list of `.js` file or directory paths, directory entries expanding alphabetically (non-recursive), relative paths resolving against the config file's directory, each file served at `/assets/user-js/<file name>` — and the `barduck:panels-updated` event: when it fires (after the re-rendered panels are in the DOM, so handlers need no deferral), what its `detail` carries, and a short `addEventListener` recipe showing a user script modifying a per-source panel (`#panel-<source>`) on each refresh.

#### Scenario: Agent configures user scripts
- **WHEN** a user asks to customize the web dashboard with JavaScript
- **THEN** the agent, guided by SKILL.md, knows to set `web_user_js` with file or directory paths and how ordering works

#### Scenario: Agent hooks panel refreshes
- **WHEN** a user asks for per-source panel customization that reacts to new values
- **THEN** the agent, guided by SKILL.md, knows to listen for `barduck:panels-updated` and query `#panel-<source>` inside the handler, modifying it directly without deferring
