## MODIFIED Requirements

### Requirement: Source summary strip
The web dashboard SHALL show a single summary strip directly under the page title, containing one chip per source that appears in a configured layout, ordered the same way panels are laid out — layouts in declaration order, then each layout's rows top-to-bottom and cells left-to-right. Each chip's color SHALL match the color its panel currently renders with: threshold band color when the source is healthy and declares threshold bands, health-derived color (`failing`→red, `stale`→yellow) when the source is failing or stale, and — since a chip is always a filled pill and needs some visible color even when its panel renders with no accent at all — a neutral gray when the source is healthy and declares no threshold bands. Clicking a chip SHALL navigate to and visually highlight that source's panel. The strip SHALL update on the same refresh cycle as the panel grid, so its colors never lag behind the panels'.

#### Scenario: Chips reflect panel colors in layout order
- **WHEN** a layout places four sources whose panels currently render green, red, green, and yellow
- **THEN** the summary strip shows four chips in that same order and coloring

#### Scenario: Unbanded healthy source's chip is gray, not green
- **WHEN** a source declares no threshold bands and is currently healthy
- **THEN** its chip renders in the neutral gray style, not green

#### Scenario: Unbanded stale source's chip is yellow
- **WHEN** a source declares no threshold bands and is currently stale
- **THEN** its chip renders in the yellow style, matching its panel's health-derived color

#### Scenario: Unbanded failing source's chip is red
- **WHEN** a source declares no threshold bands and is currently failing
- **THEN** its chip renders in the red style, matching its panel's health-derived color

#### Scenario: Clicking a chip focuses its panel
- **WHEN** the user clicks a chip
- **THEN** the page scrolls to that source's panel and the panel is visually highlighted as the current focus target

#### Scenario: Strip stays in sync with panel refresh
- **WHEN** a source's health or value changes and the panel grid refreshes to reflect it
- **THEN** that source's chip color updates in the same refresh, without a manual page reload
