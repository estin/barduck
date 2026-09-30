# Spec Delta

## ADDED Requirements

### Requirement: User-defined scripts injected into the web UI
The daemon SHALL inject one `<script>` tag per configured user JavaScript file at the end of the web UI `<body>`, after the daemon's own inline scripts, in the configured resolution order. Tags SHALL appear on every web UI page (the dashboard and per-source log views). Each tag SHALL reference a daemon-served URL for its file with `Content-Type: application/javascript`; the daemon SHALL serve user scripts only from the startup-resolved configured file set and MUST NOT expose arbitrary filesystem paths. When no user scripts are configured, or none resolve, pages SHALL render with no injected tags — unchanged from before.

Injected `<script>` tags SHALL live outside the topcoat shard render region. A shard refresh (stream-driven tick or fallback timer) SHALL NOT add, remove, re-create, or re-execute them; each injected script runs exactly once per page load. A full page load (navigation, reload) SHALL re-run them normally.

#### Scenario: Configured file is injected and loadable
- **WHEN** `web_user_js` lists one existing `.js` file and the dashboard is loaded
- **THEN** the page ends its `<body>` with exactly one `<script>` tag for that file, and fetching its URL returns the file's bytes as `application/javascript`

#### Scenario: Log views get the same scripts
- **WHEN** user scripts are configured and a per-source log view is loaded
- **THEN** the log view carries the same injected tags in the same order as the dashboard

#### Scenario: Order follows resolution order
- **WHEN** `web_user_js` lists a second file before a directory whose files sort earlier alphabetically
- **THEN** the second file's tag precedes every tag from the directory, and directory tags are in alphabetic order

#### Scenario: Shard refreshes leave scripts alone
- **WHEN** a new value is stored (triggering a shard refresh) while user scripts are injected
- **THEN** the panels re-render without a full page reload and the injected tags are neither re-created nor re-executed

#### Scenario: No scripts configured means unchanged pages
- **WHEN** `web_user_js` is absent or empty
- **THEN** the dashboard and log views contain no injected `<script>` tags

#### Scenario: Arbitrary paths are not served
- **WHEN** a request targets a user-script URL that is not in the startup-resolved set
- **THEN** the daemon does not return any file's contents
