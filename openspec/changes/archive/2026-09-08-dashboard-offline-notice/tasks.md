## 1. Shared connection-state signal

- [x] 1.1 In `CONNECTION_SCRIPT` (`src/web/routes.rs`), have `setState()` write the current state to `document.body.dataset.bdConnection` in addition to updating the dot/label. Verify by loading the dashboard and inspecting `document.body.dataset.bdConnection` in the browser (or via a test asserting the script text sets it).

## 2. Favicon reacts to offline

- [x] 2.1 In `FAVICON_SCRIPT`'s `refresh()`, check `document.body.dataset.bdConnection === 'offline'` before falling back to the `bd-status`-derived color, and use the red status color when offline. Verify by stopping the mock/backend server (or simulating a failed `/api/ping`) and confirming the favicon turns red even when the last known health was green/yellow.
- [x] 2.2 Verify recovery: once the connection comes back, the favicon returns to reflecting `bd-status` again, not stuck red. — By construction: `refresh()` only overrides to red when `bdConnection === 'offline'`; any other value (including `online` set on recovery) falls through to the normal `bd-status` branch, unconditionally, on the very next 5s tick.

## 3. Offline banner and dim

- [x] 3.1 Add a hidden-by-default banner element to the `dashboard` page (`src/web/routes.rs`), placed in normal document flow (not overlaying) so it doesn't cover existing content, styled with the `--status-red-{border,bg,fg}` tokens, stating the connection is lost.
- [x] 3.2 Add an id/hook to the existing panel-grid wrapper `<div>` so `CONNECTION_SCRIPT` can toggle `opacity-50 pointer-events-none` on it.
- [x] 3.3 Extend `setState()` to show the banner and dim the panel wrapper when state is `offline`, and hide/undim for `checking`/`online`. Verify with an integration test asserting the banner element and dim-toggle hook are present in the rendered page, and that the script text contains the toggling logic. — Added `dashboard_includes_offline_banner_and_dim_toggle` in `tests/integration.rs`; passes.
- [x] 3.4 Manually verify end-to-end: stop the backend, confirm favicon turns red, banner appears, and panel content dims within one ping interval (~5s); restart the backend and confirm all three revert without a page reload. — No working browser in this sandbox (see soften-dark-theme-contrast's archived caveat), so verified functionally instead: extracted the actual `CONNECTION_SCRIPT`/`FAVICON_SCRIPT` text from a running demo daemon's rendered page and executed it in Node against a mocked DOM/fetch, simulating online → offline → recovery. Confirmed: offline sets `dot`=red, `label`="offline", `body.dataset.bdConnection`="offline", `banner.hidden`=false, panel wrapper gains `opacity-50 pointer-events-none`, and the favicon SVG's status tile switches to `#f87171` (red) regardless of the last known health color (`#4f9e7c` green); recovery reverts every one of those cleanly. Pure visual/CSS appearance (banner placement, dim look) still hasn't been eyeballed in an actual browser — flagging as a verification gap, not a completed visual sign-off.

## 4. Verification

- [x] 4.1 Run `just ci` (or the project's full build/test/clippy/fmt command) and confirm everything passes with no regressions. — 125/125 tests pass, clippy clean. One pre-existing test (`web_ui_text_cell_renders_titled_markdown_with_no_source_extras`) needed its page-wide `--status-` check scoped past the new offline banner, which now legitimately carries `--status-red-*` styling on every page regardless of content — same pattern as the earlier favicon-script fix in soften-dark-theme-contrast.
