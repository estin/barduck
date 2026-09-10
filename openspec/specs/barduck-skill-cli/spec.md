# barduck-skill-cli Specification

## Purpose

The `barduck skill` CLI subcommand prints the embedded SKILL.md to stdout, giving LLM agents an on-demand reference for working with barduck. The SKILL.md is embedded at compile time via `include_str!` for simplified single-binary distribution.

## ADDED Requirements

### Requirement: `barduck skill` CLI subcommand

The `barduck` binary SHALL accept a `skill` subcommand that prints the embedded SKILL.md content to stdout. The SKILL.md content is embedded at compile time and SHALL NOT need to exist as a separate file in the working directory.

#### Scenario: Running `barduck skill`
- **WHEN** the user runs `barduck skill`
- **THEN** the embedded SKILL.md content is printed to stdout and the process exits with code 0

#### Scenario: `barduck skill` without SKILL.md file present
- **WHEN** the user runs `barduck skill` in a directory without a `SKILL.md` file
- **THEN** the embedded SKILL.md content is still printed correctly (file is not required at runtime)

### Requirement: SKILL.md describes agent workflow

The SKILL.md SHALL describe how an LLM agent can work with barduck to create sources by user query, understand layout configuration, and use the barduck CLI.

#### Scenario: Agent reads SKILL.md before creating a source
- **WHEN** an agent reads `SKILL.md`
- **THEN** it understands the `query` and `stream` source types, the TOML configuration format, and the available CLI commands

#### Scenario: Agent reads SKILL.md before configuring layouts
- **WHEN** an agent reads `SKILL.md`
- **THEN** it understands how layouts group sources into panels, how `show_in` controls visibility, and how `Cell` references work
