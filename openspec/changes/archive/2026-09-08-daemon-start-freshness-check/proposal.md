## Why

Today, every interval-scheduled source fires its first fetch the instant the daemon starts (`Schedule::new` seeds `next = now`), regardless of how recently that source last succeeded. A daemon restart (deploy, crash recovery, `systemctl restart`) therefore causes every source to be re-fetched at once, even sources whose last reading is only seconds old and won't be due again for a long time (e.g. a `1h` interval source restarted 5 minutes after its last success). This wastes upstream calls/quota, causes an avoidable startup burst of concurrent fetches, and produces a fetch-log entry that doesn't reflect an actual schedule due-date.

## What Changes

- On daemon startup, an interval-scheduled source's first fetch is no longer unconditional. The collector checks the source's last successful fetch (`Db::last_success`):
  - If the source has never succeeded, or its last success is already older than its `interval`, it is due — fetched immediately (unchanged from today's behavior for these cases).
  - If its last success is still within `interval`, the first fetch is deferred until the remaining freshness window elapses (`interval - age`), instead of firing immediately.
- Cron-scheduled sources are unaffected: their schedule already computes the next absolute cron occurrence on startup rather than firing immediately, so there is no startup-stampede behavior to fix for them.
- `collect_once` (used by one-shot CLI commands and the test suite) is unaffected: an explicit one-shot collection request always fetches every source immediately, regardless of freshness.
- Setup commands are unaffected in ordering: a source with a `setup` command still runs setup immediately before its first fetch *when that first fetch is due*; if the first fetch is deferred by this change, setup is deferred along with it (there is nothing useful to set up until a fetch is actually about to happen).

## Capabilities

### Modified Capabilities
- `data-collection`: the "Per-source schedules" requirement gains startup-specific behavior — an interval-scheduled source's first fetch after daemon start is scheduled based on the freshness of its last recorded success, not fired unconditionally.

## Impact

- `src/collector.rs`: `Schedule::new` becomes async and takes a `&Db` reference so it can look up the source's last success before computing its initial `next` tick; call sites in `loop_source` are updated accordingly.
- No database schema changes — reuses the existing `Db::last_success` query.
- No config changes — reuses each source's existing `interval`/`effective_interval()`.
- Tests in `tests/integration.rs` that use `spawn_all`/`spawn_graceful` against a freshly-seeded (empty) database are unaffected, since "no prior success" is treated as due-now, matching current behavior.
