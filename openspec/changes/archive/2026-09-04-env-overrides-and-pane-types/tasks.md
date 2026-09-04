## 1. Environment variable overrides

- [x] 1.1 Add an env-var override pass in `src/config.rs` covering `database_path`, `listen`, `interval`, `failure_threshold`, `stale_after`, `history_points`, `tui_width` (`BARDUCK_<FIELD>`), applied after TOML parsing and before `validate()`; verify with a unit test that a set `BARDUCK_LISTEN` overrides both an explicit file value and the field's default
- [x] 1.2 Make a parse failure (e.g. `BARDUCK_STALE_AFTER=not-a-duration`) return a `config::load()` error naming the variable and the failure, verified by a unit test
- [x] 1.3 Verify no env var set leaves existing config-file/default behavior unchanged, via a unit test run with the relevant `BARDUCK_*` vars absent

## 2. Config schema: generalized pane cell

- [x] 2.1 Change `Cell::Group` from `{ title, ids }` to `{ title, main: Option<GroupItem>, secondary: Vec<GroupItem>, table: Vec<GroupItem> }` (`#[serde(default)]` on `secondary`/`table`) in `src/config.rs`
- [x] 2.2 Update `Cell::source_names()` to collect from `main`, `secondary`, and `table`; update `Cell::pane_title()` if needed; verify via existing/updated unit tests
- [x] 2.3 Update `validate_cell` (or equivalent) to reject a `Group` where `main`, `secondary`, and `table` are all absent/empty, and to check every referenced source across all three sections, verified by unit tests for: empty pane rejected, unknown source in `main` rejected, unknown source in `secondary` rejected, unknown source in `table` rejected

## 3. Web UI rendering

- [x] 3.1 Extract the existing single-source panel rendering in `src/web.rs` into a helper reusable for both a bare `Cell::Source`/`Cell::Pane` and a `Group`'s `main` member
- [x] 3.2 Render `Group.secondary` members using that same helper in a compact/smaller-font style with history-bar rendering suppressed regardless of thresholds/`show_history`
- [x] 3.3 Keep `Group.table` members rendering via the existing table-row logic (label + value, age-only-when-stale, own history bar)
- [x] 3.4 Update the card border color computation to fold over `main` + `secondary` + `table` combined; verify with a test asserting the border reflects the worst color when the red member is in `secondary` (not just `table`)
- [x] 3.5 Add/update `src/web.rs` tests for: main-only pane renders like a single-source panel, secondary member never shows a history bar, secondary member's age is always shown, combined main+secondary+table pane renders all three sections in one card

## 4. TUI rendering

- [x] 4.1 Mirror the same `main`/`secondary`/`table` split in `src/tui.rs`: `main` and `secondary` render the value directly (no label) with always-shown age, `secondary` in the terminal's non-emphasized style; `table` keeps today's "label: value", age-only-when-stale rendering
- [x] 4.2 Update the panel border color computation to fold over all three sections
- [x] 4.3 Update the existing `group_panel_*` tests in `src/tui.rs` for the new `Cell::Group` shape and add tests for a main-only panel and a combined main+secondary+table panel

## 5. Docs and demo config

- [x] 5.1 Update `demo/config.toml` and `demo/README.md` if either declares a `Cell::Group` using `ids`, renaming to `table` (and adding a `main`/`secondary` example if it usefully demonstrates the new shape)
- [x] 5.2 Run `cargo build`, `cargo clippy --all-targets`, and `cargo nextest run`, and confirm no new failures beyond the pre-existing sandbox-only TCP test failures
