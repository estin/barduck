## Why

The web UI's per-source fetch-log view (`/logs/<source>`) always shows a fixed
window of the 50 most recent attempts and has no way to narrow it to failures.
When a source is flaky, an operator must scan the whole table to find its
errors, and there is no way to page back through older attempts or choose how
many entries show per page. The database already records an `error` on every
failed fetch-log row, so the data needed to filter to errors exists but is not
exposed by the view.

## What Changes

- **Errors-only filter (web UI):** the `/logs/<source>` view gains an "Errors
  only" toggle backed by the URL param `?error=1`. With it set, only fetch-log
  rows whose `error` is non-null are shown. Filter state is URL-driven and is
  preserved across the view's 5-second live refresh (and across back-link and
  pager navigation).
- **Pagination (web UI):** the same view gains Prev/Next controls, URL-driven
  via `?page=N` (1-based). Page size defaults to the new `logs_per_page`
  setting and falls back to 50.
- **Default entries-per-page setting:** a new top-level config field `logs_per_page`
  (default `50`) with a `BARDUCK_LOGS_PER_PAGE` env override, used as the
  default page size for log retrieval in the web UI.
- **Data layer:** `Db::logs` / `Db::logs_for_sources` (and the shared
  `query_logs`) gain an error-only filter and offset-based pagination; a
  requested page size is clamped to `MAX_LOGS_LIMIT` (10,000).

No breaking changes. `/api/logs` and the CLI `logs` command are intentionally
out of scope — see Non-Goals.

## Capabilities

### New Capabilities

None. Error filtering and pagination extend existing capabilities rather than
introducing a new one.

### Modified Capabilities

- `web-ui`: the per-source log view adds an error-only filter, pagination
  controls (Prev/Next, URL `?page=N`), and a configurable default entries-per-page.
- `data-storage`: fetch-log retrieval supports an error-only filter and
  offset/limit pagination with the limit clamped to `MAX_LOGS_LIMIT`; the
  `Config` gains `logs_per_page` (default 50, `BARDUCK_LOGS_PER_PAGE` override)
  as the default page size.

## Impact

- `src/web/routes.rs` — `source_logs` reads `error`/`page` query params;
  `page_chrome` passes page size, offset, and `errors_only` into the `log_rows`
  shard (growing its args); `log_rows` fetches the filtered/paged rows and renders
  an error toggle and a Prev/Next pager above the table; the back link preserves
  the query string.
- `src/db.rs` — `query_logs` adds an `errors_only` clause (`AND error IS NOT NULL`
  when true) and `OFFSET ?`; `Db::logs` / `Db::logs_for_sources` accept the new
  parameters; page size clamped to `MAX_LOGS_LIMIT`.
- `src/config/mod.rs` + `src/config/defaults.rs` — add `logs_per_page` to `Config`
  with `default_logs_per_page()` and the `BARDUCK_LOGS_PER_PAGE` env override,
  mirroring `history_points`.
- Tests in `src/web/routes.rs`, `src/db.rs`, and `src/config/defaults.rs`.

## Non-Goals

- Adding `?error=1`/`?page=` to the `GET /api/logs` endpoint (http-api).
- Giving the CLI `barduck logs` an error filter or `--page`/`--offset`.
- Changing the existing error column model or health computation.
- Remembering the user's last page/filter choice persistently (beyond the URL).
