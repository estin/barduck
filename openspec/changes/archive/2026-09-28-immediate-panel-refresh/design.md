# Design

## Context

See `proposal.md` (Why) for motivation. Current state: both content shards
(`panels_grid`, `log_rows`) re-render only when the browser-side `tick`
signal increments — a hardcoded `setInterval(..., 5000)` in `page_chrome`
(`src/web/routes.rs`) that bumps a topcoat `signal`. Values are stored in
three places: scheduled-poll completion and forced-poll completion in
`src/collector.rs`, and HTTP push ingest in `src/api.rs` (`POST
/api/ingest`). None of those paths currently notify connected browsers;
the 5 s tick is the only refresh trigger.

Constraint: shards re-render server-side on signal change via the existing
topcoat runtime channel (page WebSocket). That path stays — the browser
still re-renders by calling `tick.increment()` — and SSE only decides
*when* to call it.

## Goals / Non-Goals

**Goals:**

- Any stored value (all three origins) reaches connected browsers over SSE
  in well under a second and bumps `tick`, re-rendering `panels_grid` and
  `log_rows` through the existing shard mechanism.
- The periodic tick becomes fallback-only: it fires only if no SSE update
  arrived within the interval (each SSE event resets its timer).
- The fallback interval is operator-configurable (`web_refresh_interval`,
  default `5s`).
- SSE reconnects survive drops without losing updates (generation cursor)
  and without a user reload.

**Non-Goals:**

- No per-panel partial updates — full shard re-render as today, just sooner.
- No change to what panels/log rows render or to persistence.
- No cross-daemon fan-out (single daemon owns its browsers).

## Decisions

- **SSE endpoint `GET /api/refresh-events` carrying a monotonic generation
  per stored value** (rather than 1 s counter polling): the shared store
  helper bumps a `u64` generation in `AppState` and publishes it on a
  `tokio::sync::broadcast` channel; the endpoint streams
  `text/event-stream` frames (`event: refresh`, `data: {generation}`,
  plus `id:` = generation for `Last-Event-ID` resume). Rationale: true
  push latency (no 1 s poll floor), no per-browser QPS when idle, and the
  browser `EventSource` API gives reconnect for free. The poll alternative
  was the previous design — rejected per user decision: polling bounds
  staleness at its interval and burns QPS on every browser forever, while
  SSE is idle-quiet and immediate.
- **Increment + publish at the store boundary, not per origin**: the single
  helper that persists a reading (shared by poll/forced/push paths) bumps
  the counter and publishes. Rationale: one call site covers all current
  and future origins. Broadcast lagged-receiver errors are treated as
  "fall behind, catch up via fallback tick", never as store failures.
- **Generation, not per-source payload**: browsers re-render the whole
  shard and let the server decide what changed — same full-grid render the
  next tick would have done, just earlier. The generation doubles as the
  SSE `id:` cursor so a reconnecting client can compare against the last
  generation it applied; anything missed is covered by the fallback tick
  firing (its timer was not reset while the stream was down).
- **Fallback-only periodic tick**: replace the unconditional
  `setInterval(tick.increment, interval)` with a resettable timer —
  `scheduleFallback()` via `setTimeout`; every SSE `refresh` event resets
  it. If the stream errors/closes, the timer is left running so it fires
  on schedule until `EventSource` reconnects. Rationale: spec requires the
  tick to fire *only* when no stream update arrived; a resetting timeout
  is the direct implementation. Alternative (keep unconditional interval
  alongside SSE) rejected — it would double-render on every pushed value.
- **Configurable interval `web_refresh_interval: Duration`**: TOML
  humantime field on `Config` (`src/config/mod.rs`) defaulting to 5 s
  (`src/config/defaults.rs`), `BARDUCK_WEB_REFRESH_INTERVAL` env override
  following the existing `BARDUCK_<FIELD>` convention, validated as a
  positive duration. `page_chrome` renders the configured millis into the
  fallback-timer setup. Rationale: user requirement; follows the exact
  pattern of `logs_per_page`/`history_points`. Alternative (hardcoded 5 s)
  rejected per user requirement. Open detail for apply: clamp absurdly low
  values (< 1 s) or honor verbatim — recommend honoring verbatim, the
  operator owns the render-load tradeoff, but validation must reject zero.
- **Log view shares the mechanism**: it reads the same `tick` signal, so
  SSE bumps both views with no extra work.

## Risks / Trade-offs

- [Risk] Long-lived SSE connections per browser (fd + broadcast receiver
  each) → Mitigation: frames only flow on stored values; idle connections
  are kernel-cheap; broadcast channel bounded (e.g. 64) with lag → fallback
  tick, so a slow reader can't OOM the daemon.
- [Risk] Proxies buffering `text/event-stream` → Mitigation: `Cache-Control:
  no-cache`, `X-Accel-Buffering: no` headers; initial `: ping` comment
  flush on connect. Same headers the ping/favicon scripts already rely on
  for plain JSON apply here.
- [Risk] Reconnect gap (values stored while down) → Mitigation: by design
  the fallback timer fires (not reset while down) and catches up; the
  generation `id:` lets the client log/observe the gap.
- [Risk] Tests asserting exact tick/shard request counts may break →
  Mitigation: update expectations in the same change (see `tasks.md`).

## Migration Plan

Additive endpoint + script + optional config key; browsers without the new
script keep the old unconditional 5 s tick (server renders old markup until
upgraded — no, markup comes from the same binary, so no mixed-version
concern). Rollback is revert; dropping `web_refresh_interval` from config
is ignored by older binaries only if they tolerate unknown keys — verify
serde behavior in apply (deny_unknown_fields would turn rollback into a
config error).

## Open Questions

None — transport (SSE), fallback gating (reset-on-event), and interval
configurability confirmed with the user.
