## Context

The log view is `source_logs` in `src/web/routes.rs`. It renders its own full page, with a `<!DOCTYPE html>` and a `<head>`, the same way the dashboard page does. It reads rows through `Db::logs`. Each `LogRow` carries `ts` (a full date and time string) and `ts_epoch` (a number of seconds).

Four links in `src/web/panels.rs` point to `/logs/<source>` with `target="_blank"`.

Two helpers already exist and cover most of this change. `age::ago(now, ts_epoch)` turns a timestamp into relative text, for example "2m ago". `config::level_for(thresholds, value)` and `config::status_color`/`accent_color` turn a value and a source's health into a color level, the same way panels color their own value.

See proposal.md for why this change matters.

## Goals / Non-Goals

**Goals:**
- Reuse `age::ago` and the existing threshold-coloring helpers instead of writing new ones.
- Keep the page server-rendered, with no new JavaScript or client-side router.
- Make the four panel links open in the current tab and add a back link on the log page.

**Non-Goals:**
- Change `/api/logs` or `Db::logs`. The data model and JSON API stay the same.
- Add a single-page app shell. A plain link and a normal page load meet the "back button" requirement, since removing `target="_blank"` alone makes the browser's own back button work.

## Decisions

Remove `target="_blank"` from all four links in `panels.rs`. No other markup change is needed there.

Add a plain `<a href="/">← Back to dashboard</a>` link at the top of the log page, above its heading. A normal link keeps the page server-rendered and needs no new script.

Replace the TIME column's raw timestamp with `age::ago(now(), l.ts_epoch)`. If `age::ago` returns `None`, show a plain dash instead. Fetch logs never produce a zero `ts_epoch`, so this fallback is defensive only.

Color the VALUE cell the same way a panel colors its own value. Look up the source's health with `health::compute`. Pass the value and the source's thresholds to `config::level_for`. Pass that level and the health to `config::accent_color`, not `config::status_color`. `accent_color` is the health-first rule panels already use for their own text color. It returns no color for a healthy, unbanded source, instead of falling back to green. This reuses the existing rule instead of adding a second, log-view-only one.

Tighten the table for a compact layout. Use smaller cell padding and a smaller font size than the current table. Drop the fixed `max-w-xs` wrapping. Use a narrower default width so more rows fit without scrolling.

## Risks / Trade-offs

A user can reach `/logs/<source>` directly, not from a panel click. That user still gets the same page and the same back link to the dashboard.

The VALUE cell's color needs the source's current health, which the page does not compute today. Adding one `health::compute` call per page render is a small, one-time cost matching what the dashboard page already does per source.
