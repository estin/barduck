## 1. Add `logs_per_page` config setting

- [x] 1.1 Add `logs_per_page: u32` field to `Config` in `src/config/mod.rs` with
      `#[serde(default = "default_logs_per_page")]` and a doc comment; add it to the
      `Default` impl.
- [x] 1.2 Add `default_logs_per_page() -> u32` (= 50) and a `BARDUCK_LOGS_PER_PAGE`
      override branch in `apply_env_overrides_from` within `src/config/defaults.rs`.
- [x] 1.3 Reject a non-positive `logs_per_page` in `src/config/mod.rs::validate`
      (mirror how `history_points == 0` is rejected today).
      **Verify:** `cargo test` config tests — default is 50; `BARDUCK_LOGS_PER_PAGE=20`
      yields 20; `logs_per_page = 0` fails `validate`.
## 2. Data layer: error filter + offset pagination
- [x] 2.1 Add a private `query_logs_filtered(conn, sources, limit, offset, errors_only)`
      in `src/db.rs` building on `query_logs`'s SQL: append `AND error IS NOT NULL`
      when `errors_only`, and `OFFSET ?` for pagination; reuse `LOG_COLUMNS` and the
      `ts_epoch DESC, id DESC` ordering.
- [x] 2.2 Add `ReadCmd::LogsFiltered { sources, limit, offset, errors_only, reply }`
      to the `ReadCmd` enum and handle it in the async reader task (clamp `limit` to
      `MAX_LOGS_LIMIT`, clamp `offset` to ≥0), leaving `ReadCmd::Logs` for existing
      callers.
- [x] 2.3 Add `pub async fn Db::logs_filtered(&self, source: Option<&str>, limit,
      offset, errors_only) -> Result<Vec<LogRow>>` that clamps `limit`/`offset`,
      enqueues `ReadCmd::LogsFiltered` (or calls `query_logs_filtered` in direct mode),
      reusing the existing reader/direct-mode pattern.
      **Verify:** `cargo test` db tests — error-only returns only rows with a non-null
      `error`; an `offset`/`limit` window returns the correct slice newest-first;
      `limit > MAX_LOGS_LIMIT` is clamped; `offset` past the end returns an empty vec.

- [x] 3.1 Add a `LogViewQuery { page: u32, error: bool }` parser in `src/web/routes.rs`
      parsing `page`/`error` from the request query (serde_urlencoded pair loop, like
      api.rs `LogsQuery::parse`); `page` defaults to 1 and clamps to ≥1.
- [x] 3.2 In `page_chrome` (`src/web/routes.rs`), parse the query, compute
      `limit = cfg.logs_per_page`, `offset = (page - 1) * limit`, `errors_only =
      error`, and pass them as shard args to `log_rows(source, tick, limit, offset,
      errors_only)`. 
- [x] 3.3 Grow the `log_rows` shard to accept `limit`, `offset`, `errors_only`; call
      `Db::logs_filtered`; render an "Errors only" toggle and a Prev/Next pager above
      the table, with links preserving `error`/`page`; disable Prev on page 1 and Next
      when the returned page holds fewer than `limit` rows; show a "Showing X–Y" range.
      **Verify:** open `/logs/<src>` (full page), `/logs/<src>?error=1` (only failed
      rows), and `/logs/<src>?page=2` (second page); pager links retain `error=1`; the
      view still live-refreshes; `cargo test` covers `LogViewQuery` parsing.

## 4. Build & full test pass

- [x] 4.1 `cargo build` and `cargo clippy` are clean.
- [x] 4.2 `cargo test` passes for the config, db, and web routes additions.
      **Verify:** `just test` (or `cargo test`) green; `barduck daemon --config
      demo/config.toml` serves `/logs/load-average?error=1&page=1` showing only failed
      attempts with working Prev/Next.
