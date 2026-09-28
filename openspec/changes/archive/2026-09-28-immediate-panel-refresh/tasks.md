# Tasks

## 1. Server-side refresh stream

- [x] 1.1 Add a monotonic refresh generation + broadcast channel to shared state (`AppState`) and publish on every successful value store (scheduled poll, forced poll, HTTP ingest) via the shared store helper, and verify the generation advances for each origin
- [x] 1.2 Serve `GET /api/refresh-events` as `text/event-stream` (`event: refresh`, `data: {generation}`, `id:` cursor, no-buffering headers, `Last-Event-ID` resume) and verify a client receives an event per stored value with no DB read on idle
- [x] 1.3 Add the `web_refresh_interval` config knob (TOML humantime + `BARDUCK_WEB_REFRESH_INTERVAL` env override, default 5s, positive-duration validation) and verify default, file, and env sources plus rejection of zero

## 2. Browser stream-driven refresh

- [x] 2.1 Wire `EventSource` in `page_chrome` (`src/web/routes.rs`) that bumps `tick` on each `refresh` event and resets a fallback `setTimeout` rendered from the configured interval (fires only when no event arrived), and verify a stored value re-renders panels promptly without reload
- [x] 2.2 Verify the fallback fires after a dropped stream (killed connection still picks up values on the interval) and stays quiet while events flow; verify the log view picks up new fetch attempts the same way

## 3. Regression coverage

- [x] 3.1 Update/extend `tests/integration.rs` (SSE endpoint shape + resume, immediate refresh on push + poll, fallback gating, config knob) and verify with `cargo test` for the affected tests
- [x] 3.2 Run `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, and the full test suite to verify no regressions
