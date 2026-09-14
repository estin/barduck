## 1. Shared outcome type

- [x] 1.1 Add a serializable `PollOutcome` (source, success, value, ts, error) next to the collector's fetch path, with a constructor that derives it from a stored/failed attempt; verify with a unit test asserting the JSON field names the HTTP endpoint and the CLI both depend on.

## 2. Collector control channel

- [x] 2.1 Generalize `ResetHub`'s per-source `UnboundedSender<()>` into a control message with `ResetSchedule` and `PollNow(oneshot::Sender<PollOutcome>)` variants, updating `reset_channels`, `spawn_graceful`, `AppState.resets`, and the `/api/ingest` sender so existing behavior is unchanged; verify `just ci` passes with the existing ingest-resets-the-schedule tests untouched.
- [x] 2.2 Build a channel for cron-scheduled sources too (today only interval query sources get one), keeping stream and ingest sources channel-less so "no channel" means "not pollable"; verify with a unit test asserting which source kinds appear in the hub for a mixed config.
- [x] 2.3 Add the poll-now arm to `loop_source`'s `select!`: run `fetch_once`, then `schedule.advance(!success)`, then reply; verify with a test that a forced poll of an interval source stores a reading and that its next scheduled fetch is a full interval after the forced attempt, not after the original one.
- [x] 2.4 Verify a forced poll never overlaps that source's scheduled fetch — test with a source whose command blocks until released, asserting only one fetch runs at a time and the forced one completes after the in-flight one.
- [x] 2.5 Verify a cron source's next occurrence is unaffected by a forced poll, and that a failed forced poll re-arms an interval source on `retry_interval`.

## 3. HTTP endpoint

- [x] 3.1 Add `POST /api/sources/{name}/poll`: look up the source, send `PollNow`, await the reply, answer with the outcome JSON; verify with a router test that a successful poll returns `success: true` with the value and records a reading plus a `poll`-origin log entry.
- [x] 3.2 Reject unknown sources with a 4xx naming the source, and `ingest`/`stream` sources with a 4xx naming why they cannot be polled; verify both with router tests asserting nothing is recorded.
- [x] 3.3 Answer a fetch that ran and failed with a success status and `success: false` plus the error; verify with a router test over a command that exits non-zero, including that a failed fetch-log entry exists.
- [x] 3.4 Report a source whose collector task is gone (closed channel / dropped reply) as a server-side failure rather than hanging; verify with a test sending a poll against a hub whose receiver was dropped.

## 4. CLI command

- [x] 4.1 Add `Cmd::Poll` with repeatable required `-s/--source`, `--json`, and `--daemon`, cross-referencing `fetch` in its help text; verify with `Cli::try_parse_from` tests covering repeated `-s`, missing `-s`, and that `fetch` still writes nothing.
- [x] 4.2 Validate every named source up front — unknown, `ingest`, or `stream` fails naming it before any source is polled; verify with a test asserting a valid source listed alongside an invalid one is not polled.
- [x] 4.3 Add the poll path to `Backend`/`query.rs`: `--daemon` or a database held by another process routes to the HTTP endpoint; otherwise open a short-lived read-write handle and call `fetch_once` in-process. Verify the direct path with a test that polls twice in one process with no daemon (covers the read-only-probe-then-read-write sequencing risk in design.md).
- [x] 4.4 Size the daemon-mode client timeout for polls from the source's configured timeout instead of the 10 s query default, leaving reads unchanged; verify with a unit test on the timeout derivation.
- [x] 4.5 Add `cli_report::print_poll`: human-readable per-source outcomes by default, the `PollOutcome` list under `--json`, exit non-zero if any attempt failed after attempting all of them; verify with tests over mixed success/failure output in both forms.

## 5. Web panel control

- [x] 5.1 Render a poll control on each panel whose source is pollable, and none on `ingest`/`stream` panels; verify with a render test over a mixed-kind layout asserting which panels carry the control.
- [x] 5.2 Wire the control's script following the existing theme-toggle pattern: POST the endpoint, disable the control while in flight, surface a failed or unreachable poll, and re-enable afterwards; verify the control does not follow the panel's `/logs/<source>` link and does not navigate away.
- [x] 5.3 Verify a completed poll's new value reaches the panel through the existing shard tick without a full page reload.

## 6. Integration and docs

- [x] 6.1 End-to-end check with the demo config: `just demo` in one terminal, `barduck poll -s <name>` in another (routes through the daemon), then stop the daemon and repeat (writes directly); confirm both store a reading and that the dashboard panel's poll control works in a browser.
- [x] 6.2 Document `poll` in `README.md` and `SKILL.md` alongside `fetch`, making the write/no-write distinction explicit; verify the skill document renders via `barduck skill`.
- [x] 6.3 Run `just ci` (clippy `-D warnings` + nextest) and report anything left unverified.

## 7. Poll-in-progress state (web + TUI), no popup

- [x] 7.1 Add a process-local, in-memory "polling" set to `Db` (same pattern as session-band overrides): `Db::mark_polling(source) -> guard` and `Db::is_polling(source) -> bool`; verify with a unit test that the flag is visible on a clone while the guard lives and clears on drop.
- [x] 7.2 Mark a source polling for the duration of `collector::fetch_once` (the one place both a scheduled tick and a forced poll actually run the command), so both paths are covered from one spot; verify with an integration test driving a real blocking command and polling `Db::is_polling` across it.
- [x] 7.3 Add `polling: bool` to `SourceHealth`, populated from `Db::is_polling`, so it reaches every consumer (web, TUI, direct/daemon query) through the existing health query surface with no new endpoint; verify the batched (`compute_all`) and per-source (`compute`) paths agree, and that the flag is independent of `status`.
- [x] 7.4 TUI: add `polling` to `Panel`, populate it from health, and show a "(polling)" marker on single-source panel titles and group main/secondary/table lines, independent of the health-derived label/color; verify with render tests over both single-source and group-pane panels.
- [x] 7.5 Web: `Panel` gains `polling`; the poll control swaps for a plain, non-interactive "polling…" marker (no `data-bd-poll`, so it can't be clicked into a second fetch) whenever the flag is set — on single-source panels, group main rows, and the log-view control — reflecting a fetch from *any* source (this control, another viewer, or the schedule); verify with a render test asserting the marker replaces the control for a genuinely in-flight fetch and the control returns once it finishes.
- [x] 7.6 Remove the web popup/toast (`#bd-poll-note` and its script): the click handler keeps disabling its own button and swapping its label to "polling…" for immediate feedback, but posts no separate message anywhere; a failed or unreachable poll is left to show up through the source's ordinary health/fetch-log trail, same as every other fetch failure. Verify the note markup is gone and the control still can't be double-clicked into two requests.
- [x] 7.7 Run `just ci` and report anything left unverified.
