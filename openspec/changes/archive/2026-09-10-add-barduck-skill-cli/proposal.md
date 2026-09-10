## Why

LLM coding agents need a way to understand how barduck works—what sources it supports, how to configure them, and how layouts are structured—without reading through the entire codebase. A `barduck skill` CLI command that prints the SKILL.md gives agents an on-demand, structured reference they can consume at the start of any session. Embedding the SKILL.md into the binary simplifies distribution — agents get the skill content without needing the source file alongside the binary.

## What Changes

- **Add `skill` subcommand** to the `barduck` CLI that prints the embedded SKILL.md to stdout (embedded via `include_str!` at compile time)
- **Create `SKILL.md`** at the project root describing how agents work with barduck: creating sources by user query, understanding layout configuration, and using the CLI (also embedded into the binary)
- **Create `.claude/commands/barduck-skill.md`** command file so `/barduck-skill` triggers the CLI command in Claude sessions
- **Create `.pi/skills/barduck/SKILL.md`** skill file for the `.pi/skills` directory, following the existing skill pattern

## Capabilities

### New Capabilities
- `barduck-skill-cli`: The `barduck skill` CLI subcommand that outputs the embedded SKILL.md content for LLM agent consumption
- `barduck-skill-doc`: The SKILL.md document itself that describes how to create sources, configure layouts, and use barduck's CLI by user query

### Modified Capabilities
- (none)
