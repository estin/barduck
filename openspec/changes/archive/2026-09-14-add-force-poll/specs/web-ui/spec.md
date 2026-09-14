## ADDED Requirements

### Requirement: Panels can force a poll

Each single-source web panel for a pollable source SHALL offer a control that fetches that source immediately, without waiting for its schedule. So SHALL a group pane's primary (`main`) row, and every source's log view at `/logs/<source>`. A group pane's compact secondary and table rows SHALL NOT carry an inline control — those rows are deliberately dense, and each already links to its source's log view, where the control is one click away. Panels for sources that have no fetch to force — `ingest` and `stream` sources — MUST NOT offer the control anywhere.

Activating the control SHALL NOT trigger a second poll of the same source while one is in flight for that same click. Whenever a source is actually mid-fetch — polled from this control, from anywhere else, or on its own schedule — the dashboard SHALL show that state on the source's panel(s) in place of the control (spec: web-ui — Poll-in-progress is visible), replacing it once the fetch finishes. When the poll completes, the panel SHALL show the resulting value, age, and health without a full page reload, on the same refresh path panels already use.

There SHALL be no separate progress or failure popup/toast. A poll whose fetch failed, and a poll the daemon could not carry out at all, are surfaced the same way any other fetch outcome is: the source's health and fetch log reflect it (spec: web-ui — Per-source log view linked from panels), and the panel returns to its normal (non-polling) state once the attempt finishes.

The control MUST NOT navigate away from the dashboard, and MUST NOT trigger the panel's existing log-view link when activated.

#### Scenario: Panel poll refreshes the value

- **WHEN** the user activates a panel's poll control and the source's fetch succeeds with a new value
- **THEN** the panel shows the new value and a fresh "updated Xs ago" without a full page reload

#### Scenario: In-flight poll is indicated and not repeated

- **WHEN** the user activates the poll control and activates it again before that poll completes
- **THEN** the panel shows the source as polling, no second request is sent for that click, and no popup or toast appears

#### Scenario: Failed poll surfaced on the panel, not a popup

- **WHEN** the user activates the poll control and the source's fetch fails
- **THEN** no popup or toast appears; the panel returns to its normal state, and the source's health and fetch log reflect the failed attempt

#### Scenario: Unpollable sources have no control

- **WHEN** the dashboard renders a panel for an `ingest` or `stream` source
- **THEN** that panel shows no poll control, and neither does that source's log view

#### Scenario: A dense group row reaches the control through its log view

- **WHEN** a source appears only as a group pane's table row
- **THEN** that row carries no inline control, and the log view its link opens offers one

#### Scenario: Control does not open the log view

- **WHEN** the user activates a panel's poll control
- **THEN** the browser stays on the dashboard

### Requirement: Poll-in-progress is visible

Whenever a source's fetch is running — whether started by its own schedule, by this browser tab's poll control, or by any other client — every dashboard panel and log view for that source SHALL show it as currently polling, independent of the source's last known health status, in place of its poll control. This is a live shared state: it SHALL be visible to any viewer's page, not only the one that triggered it, once that viewer's next refresh reads it. The indication SHALL disappear once the fetch finishes, whatever its outcome, on the same refresh path panels already use — not held open by a separate timer.

#### Scenario: A scheduled fetch is shown as polling

- **WHEN** a source's regularly scheduled fetch is running, with nobody having clicked its poll control
- **THEN** the dashboard shows that source as currently polling

#### Scenario: Visible to every viewer

- **WHEN** one browser tab force-polls a source while a second tab has the same dashboard open
- **THEN** the second tab's next refresh also shows that source as polling

#### Scenario: Polling is independent of health

- **WHEN** a currently-healthy source is mid-fetch
- **THEN** the dashboard shows it as polling without implying its health has changed

#### Scenario: Indicator clears on completion

- **WHEN** a source's in-flight fetch finishes, successfully or not
- **THEN** the polling indication is gone from the dashboard on the next refresh
