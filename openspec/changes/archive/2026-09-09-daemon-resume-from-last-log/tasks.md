## 1. Storage lookup

- [x] 1.1 Add `Db::last_attempt` returning the newest `fetch_logs` row for a source regardless of outcome and verify with a round-trip test (success row, failure row, newest-wins ordering, none → `None`)
- [x] 1.2 Keep `Db::last_success` untouched for health derivation and verify existing health tests still pass (`cargo test health`)

## 2. Startup scheduling

- [x] 2.1 Rework `first_interval_tick` to branch on last-attempt outcome (success → remaining `interval`, failure → remaining `retry_interval`, overdue → immediate) and verify with unit tests covering fresh success defers, overdue success immediate, recent failure defers to `retry_interval`, overdue failure immediate, no history immediate
- [x] 2.2 Thread `retry_interval` through `Schedule::new` into `first_interval_tick`, leaving cron and `Schedule::advance` untouched, and verify `cargo test collector` passes
- [x] 2.3 Confirm one-shot `collect_once` still fetches immediately regardless of history and verify with the existing one-shot freshness scenario test

## 3. Verification

- [x] 3.1 Run the daemon restart scenarios end-to-end (fresh success defers, recent failure waits `retry_interval`, overdue/no-history fires now, cron waits for next occurrence) and verify observed first-tick timing matches the spec
- [x] 3.2 Run `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check` and verify all pass
