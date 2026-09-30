# Tasks

## 1. Event dispatch

- [x] 1.1 Dispatch `barduck:panels-updated` with rendered source names on each tick-driven refresh (dashboard + log views), and verify with an integration test that a stored value produces the event carrying that source's name
- [x] 1.2 Verify a listener subscribed once observes repeated refreshes (stream + fallback timer) without re-subscribing, and that pages without configured user scripts still dispatch the event

## 2. Docs

- [x] 2.1 Add the "Custom Web UI Scripts" section plus `web_user_js` settings-table row to SKILL.md (config shape, ordering, served URLs, listener recipe with `#panel-<source>` example), and verify `barduck skill` output contains the new section

## 3. Verification

- [x] 3.1 Run the project's `justfile` checks and the smallest relevant test subset, and verify no regressions; confirm every spec scenario maps to an exercised check or is reported as a manual verification
