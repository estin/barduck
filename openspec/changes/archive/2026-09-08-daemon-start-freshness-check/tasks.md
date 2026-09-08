## 1. Freshness-aware schedule construction

- [x] 1.1 Change `Schedule::new` in `src/collector.rs` to `async fn new(db: &Db, src: &SourceCfg) -> Self`, and update its interval branch to look up `db.last_success(&src.name)` and compute the initial `next` from it: due-now (`Instant::now()`) when there is no prior success or the last success is at least `effective_interval()` old, otherwise `Instant::now() + (effective_interval() - age)`. Verify with `cargo build`.
- [x] 1.2 Update the sole call site in `loop_source` to `Schedule::new(&db, &src).await`. Verify `cargo build` succeeds with no other call sites needing changes (`collect_once` does not use `Schedule`).

## 2. Test coverage

- [x] 2.1 Add a collector test proving a fresh interval-scheduled source (a seeded successful log row within `interval`) is not re-fetched immediately when its schedule is (re)built — assert no new fetch happens within a short window after `spawn_graceful`/`spawn_all`. Verify with `cargo test`.
- [x] 2.2 Add a collector test proving a stale interval-scheduled source (seeded success older than `interval`, and separately: no prior success at all) is fetched immediately on schedule construction, matching current behavior. Verify with `cargo test`.
- [x] 2.3 Add a test proving a cron-scheduled source's startup behavior is unchanged (waits for its next cron occurrence regardless of last success). Verify with `cargo test`.
- [x] 2.4 Run the full suite (`just ci` or equivalent) and confirm existing tests using `spawn_all`/`spawn_graceful` against freshly-seeded databases still pass unmodified, since "no prior success" continues to mean due-now.

## 3. Documentation

- [x] 3.1 Update `demo/README.md` if it describes the daemon's startup fetch behavior, so the walkthrough matches the new freshness-aware startup scheduling. Verify by reading the affected section after editing. — No edit needed: the README documents steady-state scheduling/staleness and "restart persists history," but never claims sources are re-fetched immediately on daemon startup, so nothing there is now inaccurate.
