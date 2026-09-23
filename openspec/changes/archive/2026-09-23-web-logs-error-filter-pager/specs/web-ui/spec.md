## ADDED Requirements

### Requirement: Log view error-only filter

The `/logs/<source>` view SHALL offer an "Errors only" filter. When the `error`
query param is set (`?error=1`), the view SHALL display only fetch-log entries
whose `error` is non-null. A visible toggle/control SHALL reflect the current
filter state and link to the corresponding `?page=1&error=1` (or page-stripped)
URL. The filter state SHALL be preserved across the view's 5-second live
refresh (re-render on tick) and across back-link and pager navigation.

#### Scenario: Errors-only view hides successful attempts
- **WHEN** a source has both successful and failed fetch attempts and the user
  requests `/logs/<source>?error=1`
- **THEN** only rows carrying a non-null error are displayed, and successful
  attempts are hidden

#### Scenario: Error filter survives live refresh
- **WHEN** the error filter is active and a new fetch attempt is recorded while
  the log view is open
- **THEN** the view remains filtered to errors within one refresh cycle, showing
  the new row only if it is also a failure

### Requirement: Log view pagination

The `/logs/<source>` view SHALL paginate fetch-log entries. Page size defaults
to `logs_per_page` (spec: data-storage — Configurable default log page size),
falling back to 50. The view SHALL render Prev/Next controls that are
URL-driven via `?page=N` (1-based; page 1 when absent or unparseable). The view
SHALL show the page's entry range. Prev is disabled on page 1; Next is disabled
when the current page contains fewer than the page size rows.

#### Scenario: Page 2 renders the second window of entries
- **WHEN** `/logs/<source>?page=2` is requested with a page size of 50 and more
  than 50 entries exist
- **THEN** the second page of 50 entries is shown, newest-first, ordered by
  `(ts_epoch DESC, id DESC)`

#### Scenario: Page size falls back to the configured default
- **WHEN** `logs_per_page` is unset in config
- **THEN** the page size is 50

#### Scenario: Invalid page falls back to page 1
- **WHEN** `?page=0` or `?page=abc` is requested
- **THEN** page 1 is shown

### Requirement: Log view navigation preserves filter and page

The log view's pager and error-filter controls SHALL preserve the other active
query params, so moving to `?page=3` while the error filter is active yields
`?page=3&error=1`. The "Back to dashboard" link SHALL clear the log view query
params, returning cleanly to the dashboard.

#### Scenario: Switching pages keeps the error filter active
- **WHEN** the error filter is active (`?error=1`) and the user clicks Next
- **THEN** the next page URL retains `error=1`
