# Spec Delta

## MODIFIED Requirements

### Requirement: Panels-updated event for user scripts
After each panel-grid shard re-render — whether triggered by a stream-driven refresh event or the fallback timer — the page SHALL dispatch a `barduck:panels-updated` `CustomEvent` on `document`. The event SHALL be dispatched only once the re-rendered content has been applied to the DOM, so a handler can read and modify the fresh panel elements synchronously, without deferring; changes a handler makes SHALL persist until the next re-render. DOM changes made by handlers SHALL NOT themselves cause the event to fire. The event's `detail` SHALL carry the names of the sources rendered in that refresh, so a listener can tell which panels changed without querying anything else first. The event SHALL fire on both the dashboard and per-source log views. Listeners subscribed once (e.g. from an end-of-body user script) SHALL observe every subsequent refresh without re-subscribing. Pages with no user scripts configured SHALL still dispatch the event.

#### Scenario: Stored value triggers the event
- **WHEN** a new value is stored for a shown source and the panels re-render
- **THEN** the page dispatches one `barduck:panels-updated` event whose detail names that source

#### Scenario: Handler sees the re-rendered DOM
- **WHEN** a listener sets an attribute on a panel link inside its `barduck:panels-updated` handler, without deferring
- **THEN** the attribute is set on the newly rendered element and stays until the next re-render

#### Scenario: Handler changes do not re-fire the event
- **WHEN** a handler modifies panel DOM
- **THEN** no additional `barduck:panels-updated` event is dispatched until the next re-render

#### Scenario: Listener survives refreshes
- **WHEN** a user script subscribes to `barduck:panels-updated` on page load and two refreshes occur
- **THEN** the listener observes both events without re-subscribing

#### Scenario: Fallback timer also fires the event
- **WHEN** no stream event arrives within the configured interval and the fallback timer re-renders the panels
- **THEN** the page dispatches `barduck:panels-updated` for that re-render

### Requirement: User-defined scripts injected into the web UI
The daemon SHALL inject one `<script>` tag per configured user JavaScript file at the end of the web UI `<body>`, after the daemon's own inline scripts, in the configured resolution order. Tags SHALL appear on every web UI page (the dashboard and per-source log views). Each tag SHALL reference a daemon-served URL for its file with `Content-Type: application/javascript`; the daemon SHALL serve user scripts only from the startup-resolved configured file set and MUST NOT expose arbitrary filesystem paths. When no user scripts are configured, or none resolve, pages SHALL render with no injected tags — unchanged from before.

Each script SHALL be served at `/assets/user-js/<name>`, where `<name>` is the file's own name, so devtools and stack traces show it. Characters outside `A-Z`, `a-z`, `0-9`, `.`, `_`, `-` SHALL be replaced with `_`. When two resolved files would get the same name, the first in resolution order SHALL keep it and each later one SHALL get a `-<n>` suffix before its extension (`n` starting at 2), with a startup warning naming the file.

Injected `<script>` tags SHALL live outside the topcoat shard render region. A shard refresh (stream-driven tick or fallback timer) SHALL NOT add, remove, re-create, or re-execute them; each injected script runs exactly once per page load. A full page load (navigation, reload) SHALL re-run them normally.

#### Scenario: Configured file is injected and loadable
- **WHEN** `web_user_js` lists one existing `.js` file and the dashboard is loaded
- **THEN** the page ends its `<body>` with exactly one `<script>` tag for that file, and fetching its URL returns the file's bytes as `application/javascript`

#### Scenario: Served under the file's own name
- **WHEN** `web_user_js` resolves `~/.config/barduck/user-js/links-new-tab.js`
- **THEN** its tag is `<script src="/assets/user-js/links-new-tab.js">`

#### Scenario: Same-named files stay distinct
- **WHEN** two directories each contain `util.js` and both are configured
- **THEN** the first resolves to `/assets/user-js/util.js`, the second to `/assets/user-js/util-2.js`, and startup logs a warning for the second

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
