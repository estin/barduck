# Tasks

## 1. Format parsing and helpers

- [x] Add and test `duration` and `elapsed` format values
- [x] Format finite non-negative seconds as compact durations; keep invalid values unchanged
- [x] Format RFC 3339 timestamps as elapsed time; keep invalid values unchanged and clamp future timestamps to `just now`

## 2. Display paths

- [x] Apply both formats in single-source and grouped web panels
- [x] Apply both formats in the CLI `latest` table and omit units for both
- [x] Keep stored values and JSON output raw

## 3. Documentation and verification

- [x] Update `SKILL.md` and this OpenSpec change
- [x] Run formatting, focused tests, clippy, and the full test suite
