# Spec Delta

## MODIFIED Requirements

### Requirement: Light/dark theme toggle
The web dashboard SHALL provide a toggle control that switches the page between its existing light and dark design-token themes. On first visit, with no stored preference, the page SHALL apply the theme matching the browser's `prefers-color-scheme`. Toggling SHALL set an explicit preference, persisted across visits, that overrides `prefers-color-scheme` from then on. The chosen theme MUST be applied before the page's first paint, so no flash of the other theme is visible. The toggle control SHALL indicate the proposed action, not the current state: its icon SHALL depict the theme the page will switch to when activated, and its accessible label SHALL name that target theme.

#### Scenario: First visit follows OS preference
- **WHEN** the browser's OS-level color scheme is dark and no theme preference has been stored yet
- **THEN** the dashboard renders in the dark theme on first load

#### Scenario: Toggle switches theme
- **WHEN** the user clicks the theme toggle
- **THEN** the dashboard immediately switches from its current theme to the other one

#### Scenario: Toggle icon previews the target theme
- **WHEN** the dashboard is rendered in the light theme
- **THEN** the toggle control shows the moon (dark) icon; and when rendered in the dark theme, it shows the sun (light) icon

#### Scenario: Toggle label names the target theme
- **WHEN** the dashboard is rendered in the light theme
- **THEN** the toggle control's accessible label names switching to the dark theme (e.g. "Switch to dark theme"); and when rendered in the dark theme, it names switching to the light theme

#### Scenario: Explicit choice persists across reloads
- **WHEN** the user has toggled to dark and then reloads the page
- **THEN** the dashboard renders in dark on reload, even if the OS-level preference is light

#### Scenario: Stored preference overrides OS preference
- **WHEN** a light preference is stored and the OS-level color scheme is dark
- **THEN** the dashboard renders in light, honoring the stored preference over the OS setting

#### Scenario: No flash of the wrong theme
- **WHEN** the stored (or OS-derived) theme is dark
- **THEN** the page's first rendered frame is already dark, not a light flash that then switches to dark
