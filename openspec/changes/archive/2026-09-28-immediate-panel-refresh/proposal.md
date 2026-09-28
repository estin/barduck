# Proposal

## Why

Dashboard panels and the log view only re-render on a fixed 5 s browser tick, so a value that arrives (scheduled poll, forced poll, or HTTP push ingest) sits invisible until the next tick fires — up to 5 s of avoidable staleness on a page whose job is showing current values.

## What Changes

- Stream refresh notifications to connected browsers over Server-Sent Events: every stored value (scheduled poll, forced poll, HTTP-ingested push) publishes an event on a new `GET /api/refresh-events` SSE endpoint, and the browser bumps its `tick` signal on each event so shards re-render immediately.
- Make the periodic tick a true fallback: the browser fires its interval refresh only when no SSE update arrived within the interval (any SSE event resets the fallback timer). The fallback still covers missed events, age-text updates, and health changes without a stored value.
- Make the fallback interval configurable via a new `web_refresh_interval` setting (humantime TOML + `BARDUCK_WEB_REFRESH_INTERVAL` env override, default `5s`), replacing the hardcoded 5 s in the page script.
- No change to what panels render — only when they re-render.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `web-ui`: the panel-grid refresh and log-view live-refresh requirements gain SSE-driven immediate re-render on new values, a fallback-only periodic tick, and a configurable fallback interval.

## Impact

- Affected code: value-store paths (`src/collector.rs` scheduled/forced poll completion, `src/api.rs` HTTP ingest via the shared store helper), shared state (`src/lib.rs` `AppState` gains a broadcast channel), new SSE endpoint (likely `src/api.rs`), config (`src/config/mod.rs` + `src/config/defaults.rs` for the new knob), browser wiring (`src/web/routes.rs` tick script → EventSource + conditional fallback).
- SSE needs reconnect handling (client resumes via `Last-Event-ID`/generation cursor; fallback tick covers gaps) — decided in design.
- `tests/integration.rs` refresh tests will need expectation updates (SSE endpoint, fallback behavior, config knob).
