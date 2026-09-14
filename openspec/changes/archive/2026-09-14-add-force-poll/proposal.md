## Why

A source only produces a new reading when its own schedule says so: an interval
source with `interval = "1h"`, or a cron source that runs at 03:00. When someone
has just fixed the thing a source measures — restarted a service, topped up an
account, repaired a broken `command` — there is no way to ask for a fresh value.
The choices today are waiting out the interval, restarting the daemon (which
re-runs every overdue source, not the one in question), or running
`barduck fetch -s <name>`, which deliberately writes nothing and so leaves the
dashboard showing the same stale value it showed before.

## What Changes

- New CLI command `barduck poll -s <name>` (repeatable, at least one required):
  runs the named sources' fetches now and stores the results exactly as a
  scheduled fetch does — reading, fetch-log entry with origin `poll`, health
  refresh, threshold override. Supports `--json`. Distinct from the existing
  `fetch`, which stays a read-only debug command that writes nothing.
- Direct mode performs the fetch and the write itself, so `poll` works with no
  daemon running. When a daemon already holds the database lock, `poll`
  transparently routes through the daemon's HTTP API, reusing the fallback
  `Backend::new` already applies to reads.
- New endpoint `POST /api/sources/{name}/poll`: asks the running daemon's
  collector task for that source to fetch now, and answers with the attempt's
  outcome.
- A forced poll is serialized with the source's own collector task rather than
  racing it, and — for an interval source — resets the schedule, so the next
  scheduled fetch lands a full interval after the forced one instead of moments
  later. This reuses and generalizes the existing per-source reset channel that
  HTTP ingest already uses for exactly this purpose.
- Web dashboard: each panel gets a poll control that triggers the endpoint for
  its source and refreshes the panel with the result.
- Sources with no fetch to force — `ingest` (HTTP push only) and `stream`
  (a continuously running process, not a per-tick fetch) — reject a forced poll
  with an error naming why, in every mode.
- New shared "polling" signal, live per source, independent of health status:
  set while a fetch (scheduled *or* forced) is actually running, read off the
  same health query surface every consumer already reads. The web dashboard and
  the TUI both show it, replacing the web panel's poll control while active.
  Visible to *every* viewer of a source, not only whoever triggered the poll —
  which is what lets the web control drop its earlier per-click popup/toast
  entirely: outcomes now show up the same way for everyone, through this shared
  state and the source's ordinary health/fetch-log trail, rather than a message
  aimed at one browser tab.

## Capabilities

### New Capabilities

None. Forced polling extends four existing capabilities rather than introducing
its own.

### Modified Capabilities

- `cli`: adds the `poll` command — its source selection, its write behavior in
  direct mode, its daemon fallback, its output, and its error cases.
- `http-api`: adds `POST /api/sources/{name}/poll`.
- `data-collection`: a forced poll is serialized with the source's scheduled
  ticks and resets an interval source's schedule, alongside the existing
  "ingested values reset interval schedules" requirement; adds the shared
  per-source "polling" signal both other capabilities read.
- `web-ui`: adds the per-panel poll control, the shared live polling
  indicator that replaces it while a fetch runs (no popup/toast), and what a
  failed or unreachable poll shows instead.
- `tui`: shows the same polling indicator per source, independent of health
  color/label. No poll control of its own — forcing a poll from the TUI stays
  out of scope.

## Impact

- `src/main.rs` — new `Cmd::Poll`.
- `src/cli_report.rs` — `print_poll`.
- `src/query.rs` — `Backend::poll`; direct mode needs a read-write `Db`, which
  `Backend::Direct` does not hold today (it is `Db::open_ro`).
- `src/collector.rs` — `ResetHub`/reset channel generalized from a bare `()`
  signal into a control message carrying either "reset schedule" (today's ingest
  behavior, unchanged) or "poll now" with a reply channel; `loop_source`'s
  `select!` gains the poll-now arm.
- `src/lib.rs` — `AppState.resets` becomes the generalized control-sender map.
- `src/api.rs` — the new endpoint; `ingest` switches to the new message type.
- `src/web/routes.rs`, `src/web/panels.rs` — panel control plus its script; the
  control swaps for a live "polling…" indicator instead of a popup.
- `src/db.rs` — process-local, in-memory "polling" set (`Db::mark_polling`/
  `Db::is_polling`), the same lifetime and locking pattern as the existing
  session-band overrides.
- `src/health.rs` — `SourceHealth` gains a `polling` field, populated from
  `Db::is_polling`, so it reaches every consumer of health data for free.
- `src/tui.rs` — `Panel` gains a `polling` field; single-source, group-main,
  and group-table rendering all show it.
- No new dependencies.
