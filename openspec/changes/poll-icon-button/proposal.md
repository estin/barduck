# Proposal

## Why

The dashboard's "fetch now" control is a four-letter text link sitting in a
panel footer next to a time-ago caption, where the surrounding text competes
with it for the eye and the action itself has no visual identity of its own.
Every other action the web UI offers (theme toggle) is an icon control, so
rendering this one as a recognizable refresh icon makes it consistent with the
rest of the page and free of the space its text label currently costs in the
dense panel footer.

## What Changes

- The idle poll control becomes an icon-only button: a circular "refresh" arrow
  drawn as an inline `<svg>` in the same `viewBox="0 0 24 24"` /
  `stroke="currentColor"` style already used by the header's theme toggle. No
  visible text label remains.
- The control keeps its `data-bd-poll` attribute, its `title`, and its
  `aria-label="Fetch <source> now"`, so hovering and assistive technology both
  still name the action; the icon itself is `aria-hidden` so it is not
  announced as a graphic.
- Icon size is matched to the `text-xs` footer it sits in, so it aligns with the
  adjacent time-ago caption instead of towering over it — the reason the control
  never used the `button` component's `ButtonSize::Icon`.
- `POLL_SCRIPT` stops rewriting the control's `textContent` on click. That swap
  exists only to echo the "polling…" state optimistically; with an inline `<svg>`
  it would destroy the icon and then restore it as escaped markup. The script now
  marks the button busy (disabled + `aria-busy` + a spin class on the icon) and
  leaves the authoritative state to the existing server-side "polling…" re-render.
- An icon-only control gains an explicit visible keyboard focus indicator, since
  it no longer has a text label to make it obviously interactive.
- The in-flight "polling…" marker stays textual — it is a state, not an action,
  and it remains the server-rendered source of truth for the live polling state.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `web-ui`: "Panels can force a poll" gains the control's icon-only
  presentation — the control is recognizable as a refresh/fetch action without a
  visible text label, carries an accessible name naming the source, keeps a
  visible keyboard focus indicator, and reflects an in-progress poll
  non-destructively (never replacing the icon with text). "Poll-in-progress is
  visible" is clarified so the textual "polling…" marker, not the icon, is what
  replaces the control while a fetch runs.

## Impact

- Affected code: `src/web/panels.rs` (`poll_button` — the icon markup, sizing,
  and `aria-hidden`), `src/web/routes.rs` (`POLL_SCRIPT` — replace the
  `textContent` save/swap/restore with a non-destructive busy state).
- No new dependencies: the icon is inline SVG markup following the theme
  toggle's existing pattern. No route, endpoint, asset, or config change; the
  delegated `data-bd-poll` click path and the three `poll_button` call sites
  are untouched.
- The existing integration tests assert on the `data-bd-poll` attribute, not on
  the "poll now" wording, and the in-flight test's `">polling…<"` marker is
  unchanged — so no test asserts the old presentation.
- Assumed icon: a two-arrow circular "refresh" glyph, chosen because the action
  is "re-fetch this source now" and because it is drawn in the same
  `viewBox="0 0 24 24"` / `stroke="currentColor"` style as the theme toggle.
  Recorded here as a reversible detail; a different glyph needs only a markup
  swap, not a spec change.
- Risk: `topcoat asset bundle` decides which utility classes are generated.
  The new classes come from the same vocabulary already present in `src/`, but
  the spin state is applied from a JavaScript string inside a Rust raw literal,
  so it needs a visual check that the class is actually emitted.
