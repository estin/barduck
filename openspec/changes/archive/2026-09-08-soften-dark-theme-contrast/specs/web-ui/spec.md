## ADDED Requirements

### Requirement: Dark theme uses moderated contrast and desaturated status colors
The dark theme SHALL use a moderated background/foreground contrast — a dark gray background rather than a near-black one, and an off-white foreground rather than stark white — instead of the widest possible light/dark contrast range. Every `--status-{red,yellow,green}-*` token used by panels, the history bar, and summary-strip chips SHALL have a dark-mode value at least as desaturated as its light-mode counterpart; none SHALL fall through unoverridden to the light theme's saturated value. Health and threshold-band level selection (which of red/yellow/green applies) is unaffected — only the color values those levels render as in dark mode change.

#### Scenario: Dark background is not near-black
- **WHEN** the dark theme is active
- **THEN** the page background renders as a dark gray tone, not a near-black tone indistinguishable from `#000000`

#### Scenario: Dark foreground is not stark white
- **WHEN** the dark theme is active
- **THEN** primary body text renders as an off-white tone, not `#ffffff` or a value perceptually equivalent to it

#### Scenario: No status color is more saturated in dark mode than in light mode
- **WHEN** a source's panel, history-bar segment, or summary-strip chip renders a red, yellow, or green status color in the dark theme
- **THEN** that color's saturation is no greater than the corresponding light-theme color's saturation

#### Scenario: Status colors remain distinguishable from each other
- **WHEN** red, yellow, and green status colors are shown together in the dark theme (for example three chips of different health/band levels)
- **THEN** each color remains clearly distinguishable from the other two by hue

#### Scenario: History bar and connection indicator follow the same theme tokens
- **WHEN** the dark theme is active
- **THEN** the panel history bar's segment colors and the connection-status indicator/favicon colors are the same desaturated dark-theme colors as the rest of the page, not fixed at their light-theme values
