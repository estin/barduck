# Design

## Context

See `proposal.md` (Why) for motivation. Current state (`src/web/routes.rs`,
`page_chrome`): the toggle button renders both icons and uses Tailwind
`dark:` visibility variants — sun visible in light mode (`dark:hidden`),
moon visible in dark mode (`hidden dark:inline-block`). That reads as
"current state". The persistence flow (`POST /api/theme` setting the
`bd_theme` cookie + `location.reload()`, `THEME_TOGGLE_SCRIPT`) already
derives the target theme from `document.documentElement.classList`, so it
is unaffected by which icon is shown.

Constraint: the server renders `<html class="dark"|"light"|"">` from the
cookie on first byte (no flash), so the icon/label choice must also be
decided server-side per render, not patched client-side after load.

## Goals / Non-Goals

**Goals:**

- Button icon and accessible label describe the theme activating on click.
- Zero change to persistence, reload, or first-paint behavior.

**Non-Goals:**

- No new themes, no system/"auto" third state, no client-side class-flip
  without reload, no tooltip styling work beyond a plain `title` attribute.

## Decisions

- **Swap the visibility classes on the two existing SVGs** (sun gets
  `hidden dark:inline-block`, moon gets `dark:hidden`) rather than
  conditional server-side rendering of one icon. Rationale: keeps the
  single static markup shape and the existing no-JS-on-load guarantee;
  the `dark:` variant already tracks the server-rendered `<html>` class.
  Alternative (render one icon per theme server-side) considered — equal
  outcome, more branching for no benefit.
- **Render the `aria-label` (and `title`) server-side from `theme_class`**:
  `dark` → "Switch to light theme", otherwise ("light" or unset, where
  CSS `prefers-color-scheme` decides) → "Switch to dark theme". Rationale:
  label must match first paint like the icon does; deriving it from the
  same `theme` value the `<html>` class uses keeps them consistent by
  construction. For the unset-cookie case the exact OS-derived theme is
  unknowable server-side, but "Switch to dark theme" is the safe default
  (matches the toggle script's own `contains('dark') ? 'light' : 'dark'`
  logic for a light-or-unknown page). Alternative (update label in
  `THEME_TOGGLE_SCRIPT` on load) considered — introduces a post-paint
  label correction; rejected.
- **No change to `THEME_TOGGLE_SCRIPT` or `src/web/theme.rs`.** The script
  already posts the opposite of the current `<html>` class; icon/label
  are pure presentation of that same decision.

## Risks / Trade-offs

- [Risk] Unset-cookie first visit: server labels "Switch to dark theme"
  while an OS-dark browser actually shows dark → label/icon mismatch until
  an explicit choice is stored → Mitigation: accepted minor edge; the
  click still toggles correctly (script reads the live DOM class), and
  from the second visit on the cookie makes server and client agree.
- [Risk] Integration tests asserting current icon visibility classes or
  the generic aria-label will fail → Mitigation: update those
  expectations in the same change (see `tasks.md`).

## Migration Plan

Not applicable — static markup change, no data or rollout steps; rollback
is revert.
