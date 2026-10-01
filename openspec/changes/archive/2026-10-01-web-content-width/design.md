# Design

## Context

See proposal.md — Why. The change clones the existing theme-preference
machinery rather than inventing a new one: `src/web/theme.rs`
(`bd_theme` cookie, `theme_class(cx)`), `POST /api/theme`
(`src/api.rs`), and the header theme toggle button in `page_chrome`
(`src/web/routes.rs`). Current shell state: both `page_chrome` wrappers
(header and `#bd-panel-wrapper` content) render `mx-auto px-6` with no
width cap (full-width); the pre-change narrow look was `max-w-5xl`.

## Goals / Non-Goals

**Goals:**
- Per-browser width choice that survives restarts and applies on first
  paint; one server-wide default for fresh browsers.
- Zero new auth/identity: the cookie is the identity, as with theme.

**Non-Goals:**
- More than two widths; per-layout or per-page widths (the shell is
  shared by dashboard and log view — one width for both).
- `localStorage`-based persistence (first-paint flash, see Risks).
- Responsive auto-switching (viewport width is not user preference).

## Decisions

1. **Mirror the theme implementation file-for-file.** `WIDTH_COOKIE`
   (`"bd_width"`) + `width_class(cx)` in `src/web/theme.rs` (or a sibling
   module if reviewers prefer the name split); `POST /api/width` in
   `src/api.rs` next to `set_theme`, same 365-day `MaxAge`, same
   400-on-unknown-token shape. Rationale: reviewers already know this
   code path; tests for theme are the template for width tests.
2. **Config key `web_content_width: String` with explicit validation**
   (reject anything but the two tokens at load, naming key + accepted
   values), documented in `SKILL.md` next to the other web keys. Default
   `"narrow"` preserves the pre-change look for existing configs
   (backward compatible, no migration).
3. **Narrow = the `max-w-5xl` capped column; wide = no cap.** Both
   `page_chrome` wrappers switch together via the same computed class
   string — one value, applied twice, so header and content can never
   disagree.
4. **Toggle = header button, icon + accessible name, sibling of the theme
   toggle.** Client script posts the *new* width (computed from current
   DOM state, like theme does — a blind server-side toggle can't
   distinguish "no cookie" from "was narrow") and swaps the wrapper
   classes in place; next tick/load the server renders it directly.

## Risks / Trade-offs

- **Cookie size/proliferation**: one more small cookie; acceptable, same
  as theme.
- **Stale narrow CSS**: removing `max-w-5xl` from the default render
  means Tailwind may drop the rule from the bundle while a narrow cookie
  still needs it. Mitigation: the class string lives in Rust source
  scanned by `@source "../src/**/*.rs"`, so the rule is generated as long
  as the literal appears in code — keep the literal, never construct it
  at runtime.
- **Shard re-renders don't touch the shell**: width lives in
  `page_chrome` (static per load), panels in shards — no interaction, no
  morph conflicts.
