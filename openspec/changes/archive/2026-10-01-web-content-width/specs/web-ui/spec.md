# Spec Delta

## ADDED Requirements

### Requirement: Server-wide default content width

The dashboard SHALL support a default content width via the optional
top-level `web_content_width` key, accepting `"narrow"` (content centered
in a capped column) or `"wide"` (panels span the full viewport width).
When absent, the default SHALL be `"narrow"`. Any other value SHALL be a
config load error naming the key and the accepted values.

#### Scenario: Default narrow when unconfigured
- **WHEN** `config.toml` has no `web_content_width` key
- **THEN** pages render the centered capped column

#### Scenario: Wide default from config
- **WHEN** `config.toml` sets `web_content_width = "wide"`
- **THEN** a first-time visitor (no width cookie) gets the full-width page

#### Scenario: Invalid value rejected
- **WHEN** `config.toml` sets `web_content_width = "medium"`
- **THEN** config load fails with an error naming `web_content_width`
  and the accepted values

### Requirement: Per-browser width override

A viewer SHALL be able to override the server default in their own
browser. The choice MUST be persisted in a `bd_width` cookie
(`"wide"`/`"narrow"`, 1-year expiry) and the server SHALL render the
matching width on every page load, so the first byte already carries the
right layout with no client-side fix-up. The cookie SHALL win over the
config default; an absent or unrecognized cookie value SHALL fall back to
the config default. The override SHALL apply to both the dashboard and
the log view, and to the header and content wrappers together (they
never disagree).

#### Scenario: Cookie overrides a wide default
- **WHEN** the config default is `"wide"` and the browser sends
  `bd_width=narrow`
- **THEN** the page renders the centered capped column

#### Scenario: Unknown cookie value ignored
- **WHEN** the browser sends `bd_width=sideways`
- **THEN** the page renders the config default

### Requirement: Width preference endpoint

`POST /api/width` SHALL accept a JSON body `{ "width": "wide" }` or
`{ "width": "narrow" }`, set the `bd_width` cookie, and echo the width
back. Any other value SHALL return 400 naming the accepted values and
set no cookie.

#### Scenario: Valid width persisted
- **WHEN** the client posts `{ "width": "narrow" }`
- **THEN** the response is 200, the `bd_width=narrow` cookie is set, and
  the next page load renders narrow

#### Scenario: Invalid width rejected
- **WHEN** the client posts `{ "width": "medium" }`
- **THEN** the response is 400 and no cookie is set

### Requirement: Header width toggle

The page header SHALL carry a width toggle control next to the theme
toggle. Activating it SHALL switch to the other width, persist the
choice through `POST /api/width`, and update the rendered page to the
new width without a full reload. The control's accessible name SHALL
state the width activating switches *to*. The toggle icon SHALL depict
the width activating switches *to*: a narrow centered column on a wide
page, a full-width frame on a narrow page.

#### Scenario: Toggle switches and persists
- **WHEN** a narrow page's toggle is activated
- **THEN** the page becomes full-width and a subsequent fresh load is
  still full-width

#### Scenario: Toggle icon reflects the target width
- **WHEN** a narrow page renders
- **THEN** the toggle shows the full-width icon, and a wide page shows
  the narrow-column icon
