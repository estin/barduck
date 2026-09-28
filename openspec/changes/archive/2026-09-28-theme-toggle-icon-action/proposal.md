# Proposal

## Why

The header theme toggle button currently renders the icon matching the active theme (sun in light mode, moon in dark mode), so the icon reads as a status indicator rather than the action clicking it performs. Users expect the control to preview its outcome — clicking the button in dark mode should offer the sun, not confirm the moon.

## What Changes

- Swap the two toggle icons so the button shows the theme it will switch to: moon icon in light mode (click → dark), sun icon in dark mode (click → light).
- Update the button's `aria-label` (and `title` tooltip, if added) to name the proposed action (e.g. "Switch to dark theme" / "Switch to light theme") instead of the generic "Toggle light/dark theme".
- Update the click-side logic only if needed to stay consistent with the swapped rendering; the `POST /api/theme` + reload persistence flow is unchanged.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `web-ui`: the "Light/dark theme toggle" requirement gains action-indicating icon/label behavior — the control previews the theme it will switch to rather than reflecting the current one.

## Impact

- Affected code: `src/web/routes.rs` (`page_chrome` toggle button markup, `THEME_TOGGLE_SCRIPT` only if label updates need it), `src/web/theme.rs` untouched (cookie/class logic unchanged).
- No API, persistence, or dependency changes; styling stays within existing Tailwind `dark:` variants.
- Integration tests in `tests/integration.rs` covering the toggle markup may need expectation updates.
