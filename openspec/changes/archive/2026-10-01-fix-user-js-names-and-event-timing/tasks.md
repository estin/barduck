# Tasks

## 1. Event timing

- [x] 1.1 Stamp each shard's hidden `#bd-status` marker with the render's tick (`data-tick`) on the dashboard and log view
- [x] 1.2 Move the `barduck:panels-updated` dispatch out of the tick `bump()` into a static page script that dispatches when the marker's tick changes, and verify with an integration test that the page carries the observer script and the stamped marker

## 2. Served names

- [x] 2.1 Serve each resolved user script under its own (sanitized) file name, suffixing duplicates with `-N` and warning, and verify with unit tests for plain names, duplicates, and sanitization
- [x] 2.2 Update integration tests to request scripts by file name

## 3. Docs and verification

- [x] 3.1 Update SKILL.md (no frame deferral; served names)
- [x] 3.2 Run `just ci` and confirm no regressions
