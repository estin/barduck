# Design

## Context

The web UI is server-rendered by topcoat: `page_chrome` (`src/web/routes.rs`) builds the only full-page shell, while `panels_grid` (`src/web/panels.rs`) and `log_rows` (`src/web/routes.rs`) are `#[shard]`s re-rendered on each tick (SSE `refresh` event or fallback timer). Browser-side, topcoat's `morph` replaces DOM strictly between the shard's `<!-- topcoat::shard::start … -->` / `<!-- topcoat::shard::end … -->` markers inside `#bd-panel-wrapper` — the four existing end-of-body inline scripts already survive every tick this way. Config is a `#[serde(deny_unknown_fields)]` struct with `Default` + `BARDUCK_*` env overrides; `PathBuf` fields resolve against `config_dir`. See proposal.md for motivation; see `specs/*/spec.md` for the behavior contract.

## Goals / Non-Goals

**Goals:**
- A config-declared set of user JS files injected once per page load, surviving all shard refreshes.
- Safe serving: browser can only fetch the startup-resolved set, nothing else.

**Non-Goals:**
- Hot-reload or editing of user scripts (resolution happens once at startup).
- Sandboxing user scripts; CSP headers; executing user scripts in the daemon (they run only in the browser).
- Per-page selection of scripts; theme/permission scoping.

## Decisions

1. **Config shape: `web_user_js: Vec<PathBuf>`, default `[]`.**
   Field on `Config` with `#[serde(default = "defaults::no_user_js")]`, added to `Default for Config`; no env override (mirrors `sources`/`layouts`, which scalars-only `BARDUCK_*` never covers). Alternatives: single `Option<PathBuf>` — rejected, cannot express "dir or files" in one value; per-entry TOML tables — rejected, overkill for a path list.

2. **Resolution at startup into `Vec<ResolvedUserScript { name, path }>` held in shared app state.**
   Each entry: file → itself; dir → non-recursive `read_dir`, filter `*.js` (extension check, ASCII case-insensitive? — no: case-sensitive `extension == "js"`), sort by file name bytes, dedupe by canonical path preserving first-occurrence order. Relative entries join `config_dir` (same as `database_path`). Failures (missing, unreadable, not-a-dir/file) warn and skip. Rationale: one resolution pass makes request handling O(1) and freezes the servable set, which the serving route and template share.

3. **Serving: new `#[route(GET "/assets/user-js/{name}")]` returning file bytes as `application/javascript`, picked up by `.discover()`.**
   `name` is an opaque per-startup key (e.g. `u0.js`, `u1.js` or sanitized `sha1(path)`), looked up in the registry — never joined to the filesystem — so `../` traversal is impossible by construction. Alternative: inline script contents into the HTML — rejected: re-sends content on every page load, bloats the shell, and any escaping bug becomes an injection hole. Alternative: serve under the file's own path — rejected: URL-encoding plus traversal risk.

4. **Placement: tags emitted in `page_chrome` after the four existing inline `<script>`s, before `</body>`.**
   Since shards only re-render between their comment markers, static-shell tags are never touched by `morph`. Classic `<script src>` (no `async`/`defer`, no `type=module`): document order = execution order, globals available to later user scripts. Same tags on both pages because both go through `page_chrome`.

5. **Warn-and-skip on bad paths, not fail.**
   A typo'd script path must not take the dashboard down. Rationale: user scripts are decoration, not data; loud failure is reserved for collection config. Warnings name the path.

## Risks / Trade-offs

- [Risk] User script throws / misbehaves and breaks page behavior (e.g. throws before daemon scripts run) → Mitigation: daemon's own scripts keep working because user tags come *after* them and each classic script's failure does not block others already executed; document that user scripts run with full page privileges.
- [Risk] Script name collisions with `bd-runtime.js` / topcoat asset routes → Mitigation: distinct `/assets/user-js/` prefix and registry-keyed names.
- [Risk] Startup-resolved files edited/deleted later → serve returns stale bytes / read error becomes 404; acceptable per non-goal of hot-reload.
- [Risk] Large user files inflate nothing server-side (served by path on demand), only per-browser download once per page load (no hashing/cache headers in v1).

## Migration Plan

No migration: additive, default-off. Deploy = ship binary; existing configs unaffected. Rollback = clear `web_user_js`.

## Open Questions

- None that change specs, approach, or tasks. Cache headers (`ETag`/`Cache-Control`) for user scripts are a later optimization, not a contract change.
