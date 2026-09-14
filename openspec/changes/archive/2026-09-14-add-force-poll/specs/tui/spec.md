## ADDED Requirements

### Requirement: Poll-in-progress is visible

The TUI SHALL show, for each rendered source, whether a fetch for it is currently running (spec: data-collection — Poll-in-progress is visible), independent of that source's health-derived color or status label. This covers a fetch started anywhere — the source's own schedule, a forced poll from the web dashboard, or the CLI's `poll` command — since the TUI has no poll control of its own (forcing a poll from the TUI is out of scope for this change). The indication SHALL disappear on the TUI's next refresh once the fetch finishes.

In daemon mode this reads the same per-source signal the web dashboard reads; in direct mode (no daemon), no collector task runs in the TUI's own process, so no source is ever shown as polling there.

#### Scenario: A scheduled fetch is shown as polling

- **WHEN** a source's regularly scheduled fetch is running while the TUI is open in daemon mode
- **THEN** that source's panel shows it as currently polling

#### Scenario: A poll forced from elsewhere is shown

- **WHEN** the web dashboard or the CLI force-polls a source while the TUI is open in daemon mode
- **THEN** the TUI's next refresh shows that source as currently polling

#### Scenario: Polling is independent of health

- **WHEN** a currently-healthy source is mid-fetch
- **THEN** the TUI shows it as polling without changing its health-derived color or status label

#### Scenario: Indicator clears on completion

- **WHEN** a source's in-flight fetch finishes, successfully or not
- **THEN** the TUI's next refresh no longer shows it as polling

#### Scenario: Direct mode shows no polling sources

- **WHEN** the TUI runs in direct mode with no daemon running
- **THEN** no source is ever shown as polling, since nothing in that process fetches on a schedule
