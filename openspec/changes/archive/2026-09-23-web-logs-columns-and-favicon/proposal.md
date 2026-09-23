# Proposal

## Why

The per-source log view (`/logs/<source>`) lists TIME, DURATION, VALUE, ORIGIN, and ERROR as five separate columns, with TIME shown at single-unit precision (e.g. "3h ago"), which is coarser than an operator needs when triaging a flaky source. Separately, the browser-tab favicon on this page always falls back to green: it reads the dashboard-wide `#bd-status` marker that only `panels_grid` renders, so a log view for a red or yellow source shows no warning in the tab at all.

## What Changes

- **Log table column order and shape:** the `/logs/<source>` table's columns become, in order: TIME, DURATION, SOURCE, VALUE. `SOURCE` is the existing ORIGIN column (`push`/`poll`) relabeled and moved to third position. The standalone ERROR column is dropped; a row with an error now shows its (wrapped, red) error text in the VALUE cell instead of the value, in place of the value/error split it had before.
- **Two-unit relative time:** the log view's TIME cell shows relative age at up to two units of precision instead of one — e.g. `1d 6h`, `1h 12m`, `12m 3s`, `45s` — dropping the second unit only when it would be zero (e.g. exactly one hour shows `1h`, not `1h 0m`) or when the age is under a minute (`45s` alone, no smaller unit exists). This is a new formatting helper; it does not change the single-unit "updated X ago" text panels already show.
- **Source-colored favicon on the log view:** the browser-tab favicon on `/logs/<source>` now reflects that one source's own status color (threshold band when healthy, health color when failing/stale, green when healthy and unbanded) instead of always falling back to green. The existing dashboard-wide favicon behavior (worst color across all panels) and the offline-red override are unchanged.

## Capabilities

### Modified Capabilities

- `web-ui`: the per-source log view's column order, the ORIGIN→SOURCE relabel, folding ERROR into VALUE, two-unit relative timestamps, and a source-scoped favicon color on that page.

## Impact

- `src/web/routes.rs` — `log_rows`: reorder/relabel table columns, drop the ERROR column and render error text in the VALUE cell, fetch the source's latest reading to compute its status color, and render a `#bd-status`-equivalent marker scoped to that source so the shared `FAVICON_SCRIPT` picks it up.
- `src/age.rs` (or a new sibling module) — a two-unit relative-time formatter alongside the existing single-unit `ago`.
- Tests in `src/web/routes.rs` and wherever the new time formatter lives.
- No database, config, or API changes; `/api/logs` and the CLI `logs` command are unaffected.
