# Tasks

## 1. Config option

- [x] 1.1 Add `web_user_js: Vec<PathBuf>` to `Config` with serde default plus `Default` impl entry, and verify `just` build passes with an existing config that omits the key
- [x] 1.2 Implement startup resolution (file-or-dir entries, `config_dir`-relative join, non-recursive alphabetic `*.js` expansion, canonical-path dedupe, warn-and-skip on missing/unreadable) and verify with a unit test covering ordering, dedupe, and the missing-path warning path
- [x] 1.3 Add a regression unit test that an unknown top-level key next to a valid `web_user_js` still fails to load, and verify the suite passes

## 2. Serving

- [x] 2.1 Add `#[route(GET "/assets/user-js/{name}")]` serving registry bytes as `application/javascript` (404 for unknown names), and verify via an integration/smoke request that a configured file downloads with the JS content type and an unregistered name does not
- [x] 2.2 Wire the resolved registry into shared app state at startup, and verify both `/` and `/logs/<source>` pages render with no injected tags when the option is empty

## 3. Injection

- [x] 3.1 Emit one `<script src>` per resolved file in `page_chrome` after the existing inline scripts, and verify dashboard HTML ends `<body>` with the tags in resolution order
- [x] 3.2 Verify a shard refresh (store a new value for a shown source) re-renders panels without a full reload while the injected tags stay untouched and un-re-executed

## 4. Verification

- [x] 4.1 Run the project's `justfile` checks and the smallest relevant test subset, and verify no regressions; confirm every spec scenario maps to an exercised check or is reported as a manual verification
