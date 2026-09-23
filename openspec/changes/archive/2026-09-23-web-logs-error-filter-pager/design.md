## Context

The web UI serves a per-source fetch-log view at `GET /logs/{source_name}`
(`src/web/routes.rs`). The `source_logs` page route renders the shared
`page_chrome` component, which invokes the `log_rows` `#[shard]` with
`(source, tick)`. That shard calls `st.db.logs(Some(&source), 50)` — a
hardcoded 50-row window, all attempts, newest-first — and renders a table with
TIME / DURATION / VALUE / ORIGIN / ERROR columns (the ERROR cell already
displays the nullable `fetch_logs.error` in red). The shard re-renders every
5 s because the page-level `tick` signal increments on a client-side interval.

DB facts (`src/db.rs`):
- `fetch_logs(id, source, ts_epoch, ts, duration_ms, error?, value?, origin)`; index
  `idx_fetch_logs_source_ts(source, ts_epoch)`; `MAX_LOGS_LIMIT = 10_000`.
- `query_logs(conn, sources, limit)` → `SELECT … FROM fetch_logs {WHERE source IN(?)} ORDER BY ts_epoch DESC, id DESC LIMIT ?`.
- `Db::logs(source, limit)` wraps `Db::logs_for_sources(sources, limit)`, which enqueues
  `ReadCmd::Logs { sources, limit, reply }` to the reader task (or calls
  `query_logs` in direct mode), clamping `limit` to `MAX_LOGS_LIMIT`.
- `logs_for_sources` is also called by `GET /api/logs` (`src/api.rs`) and the
  direct-mode `Backend` (`src/query.rs`) — so changing its signature ripples
  into http-api and cli, which are out of scope.

Config pattern (`src/config/mod.rs` + `defaults.rs`): top-level scalar settings
(e.g. `history_points`) get a `default_*()` fn, a `#[serde(default = "...")]`
field on `Config`, and a `BARDUCK_*` override in `apply_env_overrides_from`;
`validate` rejects bad values (e.g. `history_points == 0`).

## Goals / Non-Goals

**Goals:** error-only filter, Prev/Next pagination, and a configurable default
entries-per-page for the web log view only.

**Non-Goals:** extending `?error=` / `?page=` to `GET /api/logs` or the CLI
`logs` command; persisting filter/page choice beyond the URL; changing the
`error` column model or health computation; adding an index on `error`; a grand
"of N" total count (a total would add a per-refresh `COUNT` scan for a
cosmetic line — not requested).

## Decisions

- **D1 — Don't change existing DB signatures.** `Db::logs` /
  `Db::logs_for_sources` / `ReadCmd::Logs` keep their current shape so api.rs,
  query.rs, and cli_report.rs are untouched. Add a parallel
  `ReadCmd::LogsFiltered { sources, limit, offset, errors_only, reply }` and a
  public `Db::logs_filtered(source, limit, offset, errors_only) -> Vec<LogRow>`.
  Rejected alternative (extend the existing variant + update all callers) —
  would pull http-api and cli into scope and touch the health path.
- **D2 — Dedicated query.** The new path uses a dedicated `query_logs_filtered`
  rather than mutating `query_logs`/`log_row` (which `Db::logs` and the health
  paths still share), so existing callers are untouched.
- **D3 — Pager state from the returned page (no COUNT query).** Prev is disabled
  on page 1; Next is disabled when the returned page holds fewer than `limit`
  rows (the last page). The view shows a range like
  "Showing {offset+1}-{offset+count}". A total would need a `COUNT(*) OVER ()`
  scan each 5 s refresh for a cosmetic line, which this change skips as a
  non-goal (D2 keeps the query count at one per refresh).
- **D4 — Error filter SQL.** When `errors_only`, append `AND error IS NOT NULL`
  to the existing `WHERE source IN (...)` (single source in the web view). No
  new index; scans are bounded by the `MAX_LOGS_LIMIT` clamp.
- **D5 — Pagination params.** Web view uses 1-based `?page=N` (default 1,
  clamped to ≥1) — friendlier URLs than raw `?offset=`. `offset = (page-1)*size`;
  `limit = cfg.logs_per_page` (default 50), clamped to `[1, MAX_LOGS_LIMIT]`.
- **D6 — Query parsing.** A small web-local `LogViewQuery { page, error }`
  parser in `routes.rs` (serde_urlencoded pair loop, like api.rs `LogsQuery::parse`),
  but single-source (source comes from the path) and with `page`/`error` — kept
  separate from the private api.rs struct (which carries a `source` Vec
  irrelevant to the single-source view).
- **D7 — Live refresh keeps working.** `page_chrome` reads the query params once
  per request and passes `limit`/`offset`/`errors_only` as fixed shard args
  alongside `tick`; topcoat re-invokes `log_rows` on each tick with the same
  args, so filter/page persist across the 5 s refresh — matching the existing
  "log view live-refreshes" requirement and the `panels_grid(tick)` pattern.
- **D8 — Pager UX.** Prev/Next links preserve `error` and `page`. Prev disabled
  at page 1; Next disabled when the page returned fewer than `limit` rows.

## Open Questions (resolved)

- Total count via extra `COUNT` query vs none? **Resolved:** none (D3) — the
  user asked for a pager, not a total; the page-size heuristic is sufficient.
- `page` (1-based) vs `offset` param? **Resolved:** `page` (D5).
- Should the 5 s refresh recompute anything beyond the current page? **Resolved:**
  no — it re-fetches the same `(source, limit, offset, errors_only)` window,
  which is exactly the existing refresh behavior generalized to paging.

## Risks / Trade-offs

- **New reader-task enum variant:** `ReadCmd::LogsFiltered` must be handled in
  the async reader, mirroring the existing `ReadCmd::Logs` arm. Low risk, same
  pattern.
- **No index on `error`:** an error-only filter scans `fetch_logs` rows, but the
  `MAX_LOGS_LIMIT` (10,000) clamp keeps it bounded; acceptable for a
  personal/home-server tool. Documented limitation.
- **Pager on a live-refreshed view:** new attempts arriving while on page >1
  shift the window (newest-first). Accepted — consistent with "most recent
  first" and the existing refresh semantics.
