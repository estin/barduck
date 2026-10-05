# Design

## Context

See `proposal.md` (Why) for motivation. Current state:

- `poll_button` (`src/web/panels.rs:493-518`) renders the idle control as a
  plain `<button type="button" data-bd-poll=...>` whose only content is the
  text `"poll now"`. Its doc comment explains the deliberate choice: plain
  footer-sized markup, not the `button` component, whose smallest size would
  tower over the `text-xs` footer. It already carries
  `title`/`aria-label="Fetch {source} now"`.
- All three render sites (dashboard panels in `panels.rs:843` and `:886`,
  log-view heading in `routes.rs:524`) call this one component, so one markup
  edit reaches every page.
- `POLL_SCRIPT` (`src/web/routes.rs:182-205`) drives clicks via the delegated
  `data-bd-poll` listener and does
  `var label = btn.textContent; btn.disabled = true;
  btn.textContent = 'polling…'; … btn.textContent = label;` — it saves,
  replaces, and restores the control's text.
- The mid-fetch state is server-rendered: the component renders the same control
  in its busy state (`disabled`, `aria-busy="true"`, `bd-spin` on the icon, no
  `data-bd-poll` so it cannot start a second fetch), which the shard re-renders
  on the next tick; the integration suite pins the busy control and the absence
  of any visible "polling" text — never a text marker.
- The header's theme toggle (`routes.rs:310-324`) is the established inline-SVG
  icon-control pattern: `<svg viewBox="0 0 24 24" stroke="currentColor"
  stroke-width="2">`, decorated by `aria-label`/`title` on the surrounding
  button.

The `textContent` swap is the only implementation detail that cannot survive
the change untouched: applied to a button whose content is an `<svg>`, it
would destroy the icon on first click and then restore the SVG source as
escaped plain text.

## Goals / Non-Goals

**Goals:**

- The idle control renders a refresh icon and no visible text, keeping
  `data-bd-poll`, `title`, `aria-label`, disabled styling, and the no-navigate
  / no-log-link behavior.
- Click feedback stays immediate (the 5 s shard tick is too slow for that) but
  becomes non-destructive to the icon, matching the server-rendered busy
  control that lands on the next tick.

**Non-Goals:**

- No change to the `button` component, `ButtonSize::Icon`, the tick cadence,
  the `/api/sources/<name>/poll` endpoint, or any collector/polling semantics.
- The in-flight indication is the control's own busy state; no text marker, it
  is a state, not an action.
- No new dependencies, asset files, routes, or config.

## Decisions

- **Icon glyph: a two-arrow circular "refresh" mark** drawn as inline SVG
  (the Lucide `RefreshCw` path set), `viewBox="0 0 24 24"`, `fill="none"`,
  `stroke="currentColor"`, `stroke-width="2"`, `stroke-linecap="round"` —
  exactly the theme toggle's icon language. Rejected `RotateCw` (single arrow)
  as less readable at caption size, and any vendored/new icon set: inline
  markup follows the existing pattern and needs no dependency.
- **Keep the plain `<button>`, resized for icon content.** The component does
  not switch to `button(… size: ButtonSize::Icon)`: that size exists for the
  `text-xl` header, and would tower over the `text-xs` footer — the same
  reason recorded in the current doc comment. The icon takes roughly
  caption-sized classes (e.g. `size-3.5 inline-block`) plus the existing
  opacity/hover/cursor classes; exact size is confirmed visually against the
  adjacent time-ago caption and adjusted with one class if it reads too big.
- **`POLL_SCRIPT` drops the `textContent` save/swap/restore.** On click it
  sets `disabled = true`, `aria-busy = "true"`, and adds a spin/dim class to
  the icon element (`btn.querySelector('svg')`); on `finally` it removes them.
  Rationale: mutating `textContent` is what would eat the icon (and later
  restore escaped markup), while a class toggle is non-destructive. The
  `inFlight` set stays keyed by source name, so duplicate-click protection
  survives the shard replacing the button's DOM between click and `finally`,
  and disabling a detached node is a harmless no-op. The shard's
  server-rendered busy control (same classes, minus `data-bd-poll`) is the
  authoritative mid-fetch state on the next tick.
- **Focus indicator explicit.** The button gains an explicit `focus-visible`
  ring consistent with the shell's other controls, since an icon with no text
  label must still read as interactive to keyboard users.

## Risks / Trade-offs

- **Utility-class discovery.** The project's UI classes are emitted by
  `topcoat asset bundle`; the spin class is applied from a JavaScript string
  inside a Rust raw literal, which the pipeline's scanner may not see. If it
  doesn't, the fallback is still correct — the button's `disabled:` styles
  (in markup, always emitted) dim it — but the icon won't spin. Mitigated by a
  visual check in the task list (load dashboard, click, watch the spin),
  falling back to a markup-side class if the pipeline can't see the JS string.
- **Minor responsive-visual risk.** An icon in a `text-xs` footer can look
  oversized or misaligned next to the time-ago caption; this is a one-class
  fix, verified visually rather than tested.
- **Feedback is the icon, not text.** A spinning icon cannot say
  "polling…"; users learn the outcome from the refreshed panel on the
  next tick — the script's old text swap was only ever a stopgap until that
