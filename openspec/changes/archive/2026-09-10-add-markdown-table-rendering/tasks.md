## 1. Dependency

- [x] 1.1 Use `pulldown_cmark::Parser::new_ext(value, Options::ENABLE_TABLES)` instead of `Parser::new(value)` — no Cargo feature change needed (tables are controlled via `Options`, not a Cargo feature)
- [x] 1.2 Verify `cargo test` passes with the new options

## 2. Parser Configuration

- [x] 2.1 Change `pulldown_cmark::Parser::new(value)` to `pulldown_cmark::Parser::new_ext(value, Options::ENABLE_TABLES)` in `src/web/markdown.rs`
- [x] 2.2 Add `use pulldown_cmark::Options;` import to `src/web/markdown.rs`

## 3. Verification

- [x] 3.1 Verify markdown table syntax (`| H | V |`) renders as `<table>` HTML elements in web panels
- [x] 3.2 Verify existing markdown rendering (lists, emphasis, links) still works correctly
- [x] 3.3 Verify HTML escaping security posture unchanged (`<script>` tags render as literal text)

## 4. Build & Test

- [x] 4.1 `cargo build` passes with no warnings
- [x] 4.2 `cargo test` passes (181 passed, 6 ignored)
