## 1. CLI command surgery

- [x] 1.1 Remove `History` and `Health` variants, their dispatch arms, and `print_history`/`print_health` from `cli_report.rs`; verify `cargo clippy --all-targets -- -D warnings` reports no dead code and `Backend` methods used by routes remain
- [x] 1.2 Change `Fetch.source` from positional to required `--source`/`-s` and add `--json` to `Reset`; verify `barduck fetch --help` shows the flag as required and `barduck history`/`barduck health` exit non-zero as unknown subcommands

## 2. ValueFormat::Json removal

- [x] 2.1 Delete the `Json` variant, shrink `VALUE_FORMATS`, collapse `formatted_content` to the two-way branch, and delete the now-unused `json_pretty` helper; verify a config with `format = "json"` fails at load naming the unknown format
- [x] 2.2 Switch demo `status-json` to `format = "markdown"`; verify the demo config loads and both UIs render the panel

## 3. Verification

- [x] 3.1 Update CLI tests to the new shapes (flag-form fetch, reset --json, removed subcommands rejected, json format rejected) and run full `cargo test` plus `cargo fmt --check`, all green
