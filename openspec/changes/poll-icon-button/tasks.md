# Tasks

## 1. Icon-only poll control

- [x] 1.1 Rework `poll_button` in `src/web/panels.rs` to render the refresh icon (inline `<svg>` in the theme toggle's style, `aria-hidden`, caption-sized classes, explicit `focus-visible` ring) with no visible text, keeping `data-bd-poll`, `title`/`aria-label="Fetch <source> now"`, and the plain-button sizing; verify `cargo clippy --locked --all-targets -- -D warnings` passes and the rendered dashboard HTML for a pollable source contains the icon markup, the `data-bd-poll` attribute, and the accessible name with no visible "poll now" text
- [x] 1.2 Replace `POLL_SCRIPT`'s `textContent` save/swap/restore in `src/web/routes.rs` with a non-destructive busy state (`disabled`, `aria-busy`, spin/dim class on the icon, removed on `finally`), keeping the delegated listener and the source-keyed `inFlight` guard; verify by clicking the control twice in quick succession (only one POST) and confirming the icon is intact after the fetch finishes without an intervening re-render
- [x] 1.3 Extend `tests/integration.rs` to assert the control renders an icon with accessible name "Fetch cpu now" and no visible text (mirroring the existing `data-bd-poll` assertions in `web_ui_renders_poll_controls_only_for_pollable_sources`), and verify with `cargo nextest run --locked` for the poll-related tests

## 2. Verification

- [x] 2.1 Run the full gate (`just ci`) and verify no regressions: formatting, clippy, and the whole test suite pass, including the revised busy-control in-flight assertions (spinning icon, `aria-busy`, no `data-bd-poll`, no visible "polling" text)
- [x] 2.2 Smoke the change end to end via `just demo`: load the dashboard, confirm the icon aligns with the adjacent time-ago caption, confirm hover shows the tooltip and keyboard tab shows the focus ring, click the control and confirm the icon spins (no text marker) while the fetch runs, and confirm the spin class is actually emitted (if not, move it into markup-side classes)
- [ ] 2.3 Re-verify after replacing the textual "polling…" marker with the control's own busy state (`src/web/panels.rs` `poll_button`, `svg.bd-spin` spin rule in `src/web/routes.rs`): click the control on a slow source and confirm the icon alone spins (no text) while the fetch runs and the actionable control returns when it finishes; run `just ci`
