## Purpose

The `barduck skill` CLI subcommand prints the project's SKILL.md to stdout, giving LLM agents an on-demand reference for working with barduck. SKILL.md is embedded into the binary at compile time via `include_str!` for simplified single-binary distribution.

## ADDED Requirements

### Requirement: `barduck skill` CLI subcommand

The `barduck` binary must accept a `skill` subcommand that prints the embedded SKILL.md content to stdout. The SKILL.md content is embedded at compile time and does not need to exist as a separate file in the working directory.

#### Scenario: Running `barduck skill`
- **WHEN** the user runs `barduck skill`
- **THEN** the embedded SKILL.md content is printed to stdout and the process exits with code 0

#### Scenario: `barduck skill` with no arguments
- **WHEN** the user runs `barduck skill` with no additional arguments
- **THEN** the full SKILL.md content is printed verbatim from the embedded resource

#### Scenario: `barduck skill` help flag
- **WHEN** the user runs `barduck skill --help`
- **THEN** clap-generated help text is shown for the `skill` subcommand

#### Scenario: `barduck skill` without SKILL.md file present
- **WHEN** the user runs `barduck skill` in a directory without a `SKILL.md` file
- **THEN** the embedded SKILL.md content is still printed correctly (file is not required at runtime)

### Requirement: SKILL.md describes agent workflow

The SKILL.md must describe how an LLM agent can work with barduck to:
1. Create sources by user query using `name` (not `id`), understanding source types, configuration format, and validation
2. Understand layout configuration using `[[layouts]]` with `title` and `rows` of `Cell` objects, `show_in` for visibility
3. Use the barduck CLI to interact with the system
4. Follow best practices: do not inject user secrets into commands, scripts resolve relative to the config directory

#### Scenario: Agent reads SKILL.md before creating a source
- **WHEN** an agent reads `SKILL.md`
- **THEN** it understands the `query` and `stream` source types, the TOML configuration format with `name` field, and the available CLI commands

#### Scenario: Agent reads SKILL.md before configuring layouts
- **WHEN** an agent reads `SKILL.md`
- **THEN** it understands how layouts use `[[layouts]]` with `title` and `rows` containing `Cell` objects, `show_in` controls visibility (not `view`), and how `Cell` references work

#### Scenario: Agent reads best practices
- **WHEN** an agent reads `SKILL.md`
- **THEN** it knows not to inject user secrets into commands, uses `stream` type to pipe from trusted sources, and that scripts resolve relative to the config directory
