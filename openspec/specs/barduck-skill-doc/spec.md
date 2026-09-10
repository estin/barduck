# barduck-skill-doc Specification

## Purpose

The SKILL.md document serves as the canonical reference for LLM agents working with barduck, describing how to create sources, configure layouts, and use the CLI by user query. The document is embedded into the binary at compile time via `include_str!` for simplified single-binary distribution.

## ADDED Requirements

### Requirement: SKILL.md documents source creation by user query

The SKILL.md SHALL explain how an agent translates a user's natural-language request into a barduck source configuration, covering source types, configuration format, value types, validation, and layout references.

#### Scenario: User asks to add a query source
- **WHEN** a user says "add a source that shows my CPU temperature"
- **THEN** the agent, guided by SKILL.md, knows to create a `query` source with `name` (not `id`), an appropriate command, `interval`, and `unit`

#### Scenario: User asks to add a stream source
- **WHEN** a user says "stream prices from this API endpoint"
- **THEN** the agent, guided by SKILL.md, knows to create a `stream` source with `expected_interval` and the correct JSONL parsing behavior

### Requirement: SKILL.md documents layout configuration

The SKILL.md SHALL explain how layouts organize sources into visible panels using `[[layouts]]` with `title` and `rows` of `Cell` objects, and `show_in` for visibility control.

#### Scenario: Agent configures a dashboard layout
- **WHEN** an agent needs to arrange sources into a dashboard
- **THEN** the agent, guided by SKILL.md, understands how to define `[[layouts]]` with `title` and `rows` using source `name` values

#### Scenario: Agent sets visibility
- **WHEN** an agent needs to hide a source from the TUI
- **THEN** the agent, guided by SKILL.md, knows to set `show_in = "web"`

### Requirement: SKILL.md documents best practices

The SKILL.md SHALL cover security and operational best practices: do not inject user secrets into commands, scripts resolve relative to the config directory, and use `stream` type to pipe from trusted sources.

#### Scenario: User asks to add an API key source
- **WHEN** a user says "add a source that calls my API with my key"
- **THEN** the agent, guided by SKILL.md, warns against putting the API key in the command and suggests using a `stream` type from a trusted source or environment variables

#### Scenario: Agent uses relative script paths
- **WHEN** an agent configures a source with `command = "nu scripts/metric.nu"`
- **THEN** the script runs from the config file's directory, not the process launch directory

### Requirement: SKILL.md documents CLI usage

The SKILL.md SHALL describe the available `barduck` CLI commands and their arguments so agents can interact with the system programmatically.

#### Scenario: Agent queries latest values
- **WHEN** an agent needs to fetch the latest readings
- **THEN** the agent, guided by SKILL.md, knows to use `barduck latest` with appropriate flags

#### Scenario: Agent debugs a source
- **WHEN** an agent needs to test a source command without writing to the database
- **THEN** the agent, guided by SKILL.md, knows to use `barduck fetch --source <name>`
