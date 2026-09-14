## Context

See proposal.md — Why. The constraints that shape the approach:

- **One writer.** DuckDB allows a single process to hold the database file. The
  daemon holds it for its whole lifetime, so a direct-mode CLI cannot write while
  a daemon runs; `Backend::new` already probes for exactly this and falls back to
  the daemon's HTTP API for reads.
- **One task per source.** `collector::loop_source` owns each source's schedule
  and its `last_status` memory (used to record health *transitions* once). Any
  fetch performed outside that task races the scheduled one and does not move the
  schedule.
- **A reset channel already exists.** `ResetHub` gives the HTTP ingest handler an
  `UnboundedSender<()>` per interval-scheduled query source; sending on it makes
  `loop_source` re-arm its wait from now (spec: data-collection — Ingested values
  reset interval schedules). A forced poll is the same shape of message with a
  fetch attached, so this is a generalization, not a second mechanism.
- **`fetch` already exists** as a deliberately write-free debug command. Forced
  poll is the writing sibling; the two must not collapse into one command.

## Goals / Non-Goals

**Goals:**

- One outcome type shared by the HTTP endpoint, the daemon-mode CLI, the
  direct-mode CLI, and the web panel, so all four report a poll identically.
- A forced poll in the daemon takes the same code path as a scheduled fetch —
  same logging, same health transition bookkeeping, same threshold handling — by
  running inside the source's collector task rather than beside it.
- No new dependency, no new long-lived task, no per-source lock beyond the
  channel that already exists.

**Non-Goals:**

- Any TUI affordance (needs panel focus/selection the TUI does not have).
- A poll-everything command or control.
- Queuing or coalescing repeated poll requests beyond "one fetch of a source at a
  time"; a second request simply waits its turn behind the first.
- Authentication for the new endpoint — it inherits the listen-address trust
  model every other endpoint has (spec: http-api — HTTP ingest endpoint).

## Decisions

### The collector task performs the poll, reached by a control channel

`ResetHub`'s per-source `UnboundedSender<()>` becomes a sender of a control
message with two variants: reset-the-schedule (today's ingest behavior, byte-for-
byte unchanged) and poll-now carrying a `tokio::sync::oneshot::Sender` for the
outcome. `loop_source`'s `select!` gains an arm for it: on poll-now it runs the
same `fetch_once` the scheduled path runs, then `schedule.advance(!success)` and
replies. Because the arm and the scheduled tick live in the same `select!` on one
task, a poll can never overlap that source's own fetch, and the schedule
naturally restarts from the forced attempt. The existing "drop the receiver when
every sender is gone" handling carries over unchanged — it is what keeps the
`select!` from spinning once `AppState` is dropped.

The channel is currently built only for interval-scheduled query sources. It must
also be built for cron sources, which are pollable; their `advance` is already a
no-op, so a forced poll correctly leaves the next cron occurrence alone. Stream
and ingest sources still get no channel — they have nothing to fetch — which
makes "no channel" the single source of truth for "not pollable" at the API edge.

*Alternatives considered.* Have the HTTP handler call `collector::fetch_once`
itself: rejected — it races the scheduled fetch, cannot move the schedule, and
has no access to the task's `last_status`, so it would double-record health
transitions. A per-source `Mutex` around fetching: rejected — it fixes only the
overlap, not the schedule, and adds a second synchronization mechanism next to
the channel that already exists.

### Direct mode fetches in the CLI process; the lock decides which path runs

`poll` resolves its path the way `Backend::new` already resolves reads: if
`--daemon` is passed, or the database file is held by another process, send
`POST /api/sources/{name}/poll`; otherwise open the database read-write in the
CLI process and call `collector::fetch_once` directly. No collector task exists
in that process and the file lock excludes every other one, so there is nothing
to serialize against.

`Backend::Direct` holds a *read-only* handle, and must keep holding one — the TUI
and the query commands must not take a write lock. So the poll path opens its own
short-lived read-write handle rather than widening `Backend::Direct`.

*Alternative considered.* Require a running daemon (route everything over HTTP):
rejected — `barduck` is usable as a pure CLI against the database file, and a
command that only works under a daemon would be the only one of its kind.

### One `PollOutcome` type, serialized identically everywhere

A single serde struct — source, success flag, value, timestamp, error — is what
the collector's poll arm returns, what the endpoint's body carries, what the
daemon-mode CLI deserializes, and what the direct-mode CLI builds locally. The
`--json` output is that struct (a list of them). This is why "the fetch ran and
failed" is a 200 with `success: false` rather than a 5xx: the CLI has to tell a
failed *fetch* from an unreachable *daemon*, and a shared body shape is the
cheapest way to preserve that distinction across both transports.

### The daemon-mode client timeout must cover the fetch

`DAEMON_REQUEST_TIMEOUT` is 10 s, chosen for queries. A forced poll is bounded by
the source's own configured timeout plus any in-flight fetch it waits behind, so
the poll request needs a timeout derived from the source's timeout instead of the
query default. Reads keep the 10 s default.

### The web control is plain browser JS, not a shard argument

The dashboard already refreshes by bumping a `tick` a shard re-renders from. A
shard argument is a *render* trigger, so an action must not be modeled as one — a
re-render for any other reason would re-fire the poll. The control instead
follows the `THEME_TOGGLE_SCRIPT` pattern: a click handler `fetch`es the endpoint,
disables the control while in flight, and lets the existing tick pick up the new
value. The handler must `preventDefault`/`stopPropagation` so it never activates
the panel's time-ago link to `/logs/<source>`.

## Risks / Trade-offs

- **A forced poll waits behind an in-flight fetch of the same source** → That is
  the point (no overlapping fetches), but it means an HTTP poll can take up to
  roughly two source timeouts. Bounded, and the timeout decision above sizes the
  client for it.
- **Opening a read-write handle in a process that already opened a read-only one**
  → DuckDB keeps one database instance per file per process; probing with a
  throwaway read-only handle and then opening read-write must be sequenced so the
  probe handle is dropped first. Worth an explicit test: poll in direct mode with
  no daemon, then poll again.
- **Direct-mode threshold overrides are process-local** → Session bands live in an
  in-memory map, so an override carried by a direct-mode forced poll vanishes when
  the command exits, exactly as it already does for `collect_once`. Not a
  regression; documented rather than fixed here.
- **Unauthenticated endpoint that runs a configured command** → The command is
  already running on the daemon's schedule; the endpoint changes *when*, not
  *what*. Still, it converts a read-only-looking surface into one that triggers
  process execution, which is a reason to keep the existing non-loopback warning
  prominent rather than to add auth here.
- **Two more things to keep in step** → `fetch` (debug, writes nothing) and `poll`
  (writes) will be confused with each other. Mitigated by each command's help text
  naming the other.
