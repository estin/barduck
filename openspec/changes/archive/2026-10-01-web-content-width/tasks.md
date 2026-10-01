# Tasks

## 1. Config plumbing

- [x] 1.1 Add optional `web_content_width` key to `Config` (default `"wide"`)
- [x] 1.2 Validate the value in `src/config/validation.rs`: reject anything but `wide`/`narrow` with an error naming the key and accepted values
- [x] 1.3 Document the key in `SKILL.md` alongside the other web settings
- [x] 1.4 Add config unit tests: default wide, narrow accepted, invalid rejected

## 2. Server-side width resolution

- [x] 2.1 Add `WIDTH_COOKIE` (`"bd_width"`) and `width_class(cx)` (cookie wins, else config default, else wide on unknown value), mirroring `src/web/theme.rs`
- [x] 2.2 Apply the computed width class to both `page_chrome` wrappers in `src/web/routes.rs` (header + content together; narrow restores `max-w-5xl`)
- [x] 2.3 Add `POST /api/width` in `src/api.rs` mirroring `set_theme`: 200 + 1-year cookie on valid token, 400 + no cookie otherwise

## 3. Header toggle

- [x] 3.1 Add width toggle button next to the theme toggle; client script posts the newly selected width (from current DOM state) and swaps wrapper classes without reload
- [x] 3.2 Accessible name states the width activating switches to; verify no first-paint flash with/without cookie

## 4. Verification

- [x] 4.1 Integration tests: default wide shell, narrow-by-config, cookie-overrides-config, unknown cookie falls back, endpoint 200/400, toggle persists across loads
- [x] 4.2 Run `just ci` (fmt, clippy, nextest) — clean
- [x] 4.3 Smoke-run with a real config: screenshot wide and narrow at 1920px, confirm no horizontal overflow and both wrappers agree on both pages (dashboard + log view)
- [x] 4.4 Confirm Tailwind bundle still contains the `max-w-5xl` rule (narrow must work from a fresh bundle)
