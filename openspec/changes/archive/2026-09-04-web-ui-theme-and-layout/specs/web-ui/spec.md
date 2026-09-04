## ADDED Requirements

### Requirement: Light/dark theme toggle
The web dashboard SHALL provide a toggle control that switches the page between its existing light and dark design-token themes. On first visit, with no stored preference, the page SHALL apply the theme matching the browser's `prefers-color-scheme`. Toggling SHALL set an explicit preference, persisted across visits, that overrides `prefers-color-scheme` from then on. The chosen theme MUST be applied before the page's first paint, so no flash of the other theme is visible.

#### Scenario: First visit follows OS preference
- **WHEN** the browser's OS-level color scheme is dark and no theme preference has been stored yet
- **THEN** the dashboard renders in the dark theme on first load

#### Scenario: Toggle switches theme
- **WHEN** the user clicks the theme toggle
- **THEN** the dashboard immediately switches from its current theme to the other one

#### Scenario: Explicit choice persists across reloads
- **WHEN** the user has toggled to dark and then reloads the page
- **THEN** the dashboard renders in dark on reload, even if the OS-level preference is light

#### Scenario: Stored preference overrides OS preference
- **WHEN** a light preference is stored and the OS-level color scheme is dark
- **THEN** the dashboard renders in light, honoring the stored preference over the OS setting

#### Scenario: No flash of the wrong theme
- **WHEN** the stored (or OS-derived) theme is dark
- **THEN** the page's first rendered frame is already dark, not a light flash that then switches to dark

### Requirement: Panel title rendered on the card border
Each panel/pane card SHALL render its configured title embedded in the card's own top border, aligned to the top-left with padding on either side of the title text — the same convention the TUI already uses for a bordered panel's title — instead of as a separate header row above the panel's content. The border line SHALL pass through the vertical center of the title text (the way the TUI's own bordered-box title sits on its border line), not above or below it.

#### Scenario: Single-source panel title on the border
- **WHEN** a layout cell is a plain source reference or a `{ id, title }` override
- **THEN** its card's title renders on the card's own top border, top-left, not in a separate header row above the content

#### Scenario: Generalized pane title on the border
- **WHEN** a layout cell is a generalized pane combining `main`/`secondary`/`table` sections (spec: source-configuration — UI layouts are config-declared like sources)
- **THEN** its card's title renders the same way, on the card's own top border

#### Scenario: Untitled generalized pane shows no border title
- **WHEN** a generalized pane cell declares no `title` and no `main` (so it renders with no header text, spec: source-configuration — UI layouts are config-declared like sources)
- **THEN** its card's top border shows no title text, matching having no header text before this change

#### Scenario: Title vertically centered on the border line
- **WHEN** any panel/pane card renders its title on the border
- **THEN** the border line bisects the title text vertically, through its center, rather than sitting above or below it

### Requirement: Compact panel density
The panel grid's cards and rows, and the source summary strip's chips, SHALL use denser spacing and a smaller type scale than before this change, so more panels and chips fit on screen without scrolling, while every piece of information a panel showed before this change (value, unit, status label, "updated X ago" text, history bar) remains visible — this change reduces spacing only, it does not hide or remove any previously shown information.

#### Scenario: Summary strip chips are compact
- **WHEN** the source summary strip renders its chips
- **THEN** each chip uses a small pill shape with tight padding, noticeably more compact than before this change

#### Scenario: All previously shown information remains visible
- **WHEN** a panel that showed a value, unit, status label, age text, and history bar before this change is rendered after it
- **THEN** all of those same pieces of information are still shown, just in a denser layout

#### Scenario: More panels fit per screen
- **WHEN** the same layout is rendered before and after this change at the same viewport size
- **THEN** the panel grid after this change occupies less vertical space per panel than before

### Requirement: Responsive layout for small viewports
The web dashboard SHALL render usably on small (phone-width) viewports. The page SHALL declare a viewport meta tag so mobile browsers render it at device width instead of a zoomed-out desktop layout. The panel grid's column count SHALL reduce as the viewport narrows — down to a single column at phone widths — instead of forcing the layout's configured column count regardless of viewport width. No element SHALL force the page to scroll horizontally at any viewport width down to a small phone width (360px).

#### Scenario: Viewport meta tag present
- **WHEN** the dashboard page is loaded
- **THEN** its `<head>` declares a viewport meta tag setting the layout width to the device width

#### Scenario: Grid collapses to one column on a narrow viewport
- **WHEN** a 3-column layout is viewed at a phone-width viewport
- **THEN** the panel grid renders as a single column, each panel spanning the full available width

#### Scenario: Wide viewport keeps the configured column count
- **WHEN** the same 3-column layout is viewed at a desktop-width viewport
- **THEN** the panel grid renders with 3 columns, as configured

#### Scenario: No horizontal overflow at phone width
- **WHEN** the dashboard is viewed at a 360px-wide viewport
- **THEN** no element (grid, card, summary strip, header) causes the page to scroll horizontally
