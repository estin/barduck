## Context

Barduck is a single-binary home dashboard with a clap-based CLI. The `Cmd` enum in `src/main.rs` currently defines `Daemon`, `Tui`, `Latest`, `Logs`, `Reset`, and `Fetch` subcommands. The project already has `.claude/commands/opsx/` and `.pi/skills/` directories for agent tooling. The `config/` module handles TOML-based source and layout configuration with `query` and `stream` source types.

See proposal.md - Why for motivation.

## Goals / Non-Goals

**Goals:**
- Add a `barduck skill` CLI subcommand that prints SKILL.md to stdout
- Embed SKILL.md into the binary via `include_str!` for simplified single-binary distribution
- Create a SKILL.md at the project root that documents agent workflow for source creation and layout configuration
- Add `.claude/commands/barduck-skill.md` for Claude agent integration
- Add `.pi/skills/barduck/SKILL.md` for the `.pi` skills directory

**Non-Goals:**
- Modifying the source collection, storage, or rendering logic
- Adding new CLI commands beyond `skill`
- Changing the config format or validation rules

## Decisions

### SKILL.md location
- **Decision**: Place SKILL.md at the project root (`SKILL.md`)
- **Rationale**: Lives in the repo for editing and version control; embedded into the binary at compile time via `include_str!`. Agents can also read it directly from the repo if they have source access
- **Alternative considered**: Keeping it only in `.claude/skills/` — rejected because it limits visibility and the CLI command needs a stable path

### `barduck skill` implementation
- **Decision**: The `skill` subcommand embeds SKILL.md at compile time using Rust's `include_str!` macro
- **Rationale**: Single-binary distribution — no need for the SKILL.md file to exist alongside the binary. `include_str!` is a zero-cost built-in macro with no additional dependencies
- **Alternative considered**: Reading SKILL.md from the filesystem at runtime — rejected because it requires the file to be present alongside the binary, complicating distribution

### `.claude/commands/barduck-skill.md`
- **Decision**: Create a new command file that wraps the `barduck skill` invocation
- **Rationale**: Follows the existing `.claude/commands/opsx/` pattern; agents can use `/barduck-skill` to print the skill doc

### `.pi/skills/barduck/SKILL.md`
- **Decision**: Create `.pi/skills/barduck/SKILL.md` as a copy of the root SKILL.md for the `.pi` skill system
- **Rationale**: The `.pi` skill system expects skill files in its directory structure. The binary embeds the content, but the `.pi` skill file allows agent discovery without running the CLI
- **Alternative considered**: Skipping `.pi/skills` entirely since the binary embeds it — rejected because the `.pi` skill system provides additional agent discovery paths

### SKILL.md content scope
- **Decision**: Cover source types, configuration format, layout structure, CLI commands, and the agent workflow for translating user queries into configurations, including:
  - Source fields use `name` (not `id`), `show_in` (not `view`), `unit`, `format`, `show_history`
  - Layouts use `[[layouts]]` with `title` and `rows` (2D array of `Cell` objects)
  - Best practices: do not inject user secrets into commands, scripts resolve relative to the config directory
  - Working directory: barduck chdirs to the config file's directory at startup
- **Rationale**: Agents need to understand the full workflow without reading source code, including security and operational guidance

## Risks / Trade-offs

- **SKILL.md staleness**: The SKILL.md must be kept in sync with the actual codebase. Since it is embedded at compile time, a rebuild is required to update the skill content. Mitigation: rebuild the binary when SKILL.md changes; the `barduck skill` command is the canonical distribution channel.
- **Binary size**: The SKILL.md adds a small amount to the binary. This is acceptable for a reference document.
