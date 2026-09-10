## 1. SKILL.md Document

- [x] 1.1 Create `SKILL.md` at project root describing how agents work with barduck: source types (`query`/`stream`), TOML configuration format, layout structure (cells/groups/views), CLI commands, and the workflow for creating sources by user query
- [x] 1.2 Verify `SKILL.md` is syntactically correct and covers all required scenarios from specs/barduck-skill-doc/spec.md

## 2. CLI `skill` Subcommand

- [x] 2.1 Add `skill` variant to the `Cmd` enum in `src/main.rs`
- [x] 2.2 Implement the `skill` subcommand handler using `include_str!("SKILL.md")` to embed the content at compile time and print it to stdout
- [x] 2.3 Add `skill` to the main dispatch logic so it executes correctly

## 3. Claude Command Integration

- [x] 3.1 Create `.claude/commands/barduck-skill.md` wrapping the `barduck skill` invocation with appropriate frontmatter
- [x] 3.2 Verify the command file follows the same format as existing `.claude/commands/opsx/` files

## 4. Pi Skills Integration

- [x] 4.1 Create `.pi/skills/barduck/SKILL.md` as a copy of the root SKILL.md for the `.pi` skill system
- [x] 4.2 Verify the skill file is discoverable by the `.pi` skill system

## 5. Verification

- [x] 5.1 Run `cargo build` to verify the CLI compiles and `include_str!` resolves correctly
- [x] 5.2 Run `barduck skill` to verify it prints the SKILL.md content from the embedded binary
- [x] 5.3 Run `barduck skill --help` to verify clap help output
