## Purpose

The SKILL.md document serves as the canonical reference for LLM agents working with barduck, describing how to create sources, configure layouts, and use the CLI by user query. SKILL.md is embedded into the binary at compile time via `include_str!` for simplified single-binary distribution.

## ADDED Requirements

### Requirement: SKILL.md documents source creation by user query

The SKILL.md must explain how an agent translates a user's natural-language request into a barduck source configuration, covering:

1. **Source types**: `query` (oneshot command on a schedule) and `stream` (continuous command emitting JSON lines)
2. **Configuration format**: TOML with `[[sources]]` tables, each with `type`, `name`, `command`, `interval`/`cron`, `timeout`, and optional fields
3. **Key fields**: `name` (not `id`), `title`, `unit`, `format`, `show_in`, `value_type`, `thresholds`, `show_history`, `setup`, `retry_interval`
4. **Value types**: `string`, `bigint`, `double`, `json` for how fetched values are stored
5. **Validation**: How to validate a source configuration before applying it
6. **Layout references**: How layouts group sources into panels using `rows` and `Cell` objects
7. **Working directory**: Scripts resolve relative to the config file's directory

#### Scenario: User asks to add a CPU temperature source
- **WHEN** a user says "add a source that shows my CPU temperature"
- **THEN** the agent, guided by SKILL.md, knows to create a `query` source with `name` (not `id`), an appropriate command, `interval`, and `unit`

#### Scenario: User asks to add a JSON API stream
- **WHEN** a user says "stream prices from this API endpoint"
- **THEN** the agent, guided by SKILL.md, knows to create a `stream` source with `expected_interval` and the correct JSONL parsing behavior

### Requirement: SKILL.md documents layout configuration

The SKILL.md must explain how layouts organize sources into visible panels, covering:

1. **Layout structure**: `[[layouts]]` with `title` and `rows` (a 2D array of `Cell` objects)
2. **Cell types**: bare strings, `{ main = "src" }`, `{ table = [...] }`, `{ kind = "space", colspan = N }`, `{ main = "src", secondary = [...], title = "Label" }`
3. **Visibility**: `show_in` controls which UI surfaces display a source (`all`, `tui`, `web`)
4. **Threshold bands**: How color bands map to numeric values

#### Scenario: Agent configures a dashboard layout
- **WHEN** an agent needs to arrange sources into a dashboard
- **THEN** the agent, guided by SKILL.md, understands how to define `[[layouts]]` with `title` and `rows` using source `name` values

#### Scenario: Agent sets visibility
- **WHEN** an agent needs to hide a source from the TUI
- **THEN** the agent, guided by SKILL.md, knows to set `show_in = "web"`

### Requirement: SKILL.md documents best practices

The SKILL.md must cover security and operational best practices:

1. **Do not inject user secrets into commands** — use `stream` type to pipe from trusted sources or HTTP ingest APIs
2. **Scripts resolve relative to the config directory** — barduck chdirs to the config file's directory at startup
3. **Commands run in barduck's process** — be cautious about what commands are configured

#### Scenario: User asks to add an API key source
- **WHEN** a user says "add a source that calls my API with my key"
- **THEN** the agent, guided by SKILL.md, warns against putting the API key in the command and suggests using a `stream` type from a trusted source or environment variables

### Requirement: SKILL.md documents CLI usage

The SKILL.md must describe the available `barduck` CLI commands and their arguments so agents can interact with the system programmatically.

#### Scenario: Agent queries latest values
- **WHEN** an agent needs to fetch the latest readings
- **THEN** the agent, guided by SKILL.md, knows to use `barduck latest` with appropriate flags

#### Scenario: Agent debugs a source
- **WHEN** an agent needs to test a source command without writing to the database
- **THEN** the agent, guided by SKILL.md, knows to use `barduck fetch --source <name>` (using `name`, not `id`)
