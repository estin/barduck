# Proposal

## Why

Sources can report durations as seconds or timestamps. These values need different display rules, so their format names must state what the input means.

## What Changes

- Add `duration` to format numeric seconds as a human-readable duration such as `1d 2h`.
- Add `elapsed` to parse an RFC 3339 timestamp and show the time since that timestamp, such as `3h 24m`.
- Apply both formats in single-source and grouped web panels and in the CLI `latest` table. Omit `unit` for these formats.
- Show invalid values unchanged. Show future elapsed timestamps as `just now`.
- Keep stored values and `latest --json` output unchanged.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `source-configuration`: define `duration` and `elapsed` value formats.

## Impact

- Affected code: value-format parsing, shared duration formatting, web panels, CLI output, and `SKILL.md`.
- No new dependencies are needed. The project already uses `chrono` and `humantime`.
- Existing format values remain valid.
