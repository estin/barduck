# Spec Delta

## MODIFIED Requirements

### Requirement: Config file defines sources
The system SHALL read all source definitions from a single user config file at startup. Each source MUST have a unique name, a type (`query` or `stream`), type-specific parameters, and a schedule selector: a `query` source is scheduled by `interval` or `cron`, while a `stream` source declares `expected_interval` instead of a schedule. The config file SHALL resolve by this precedence: an explicitly passed `--config`/`-c` path first; else the `BARDUCK_CONFIG` environment variable when set and non-empty; else `barduck/config.toml` under the XDG config home (`$XDG_CONFIG_HOME` when set and non-empty, otherwise `$HOME/.config`) when that file exists; else `./config.toml` in the working directory when it exists. When none of these yields an existing file, startup MUST fail with an error naming the paths tried. On every successful startup the system SHALL print the used config path to stderr.

#### Scenario: Valid config loads
- **WHEN** the config file contains one `query` source and one `stream` source with valid parameters
- **THEN** both sources are registered and eligible for collection

#### Scenario: Invalid source rejected
- **WHEN** a source definition is missing required fields or has an unknown type or duplicate name
- **THEN** startup fails with an error naming the offending source and field

#### Scenario: Default resolves under XDG_CONFIG_HOME
- **WHEN** `XDG_CONFIG_HOME=/home/user/.cfg` is set, no `--config` or `BARDUCK_CONFIG` is given, and `/home/user/.cfg/barduck/config.toml` exists
- **THEN** startup reads `/home/user/.cfg/barduck/config.toml`

#### Scenario: Default falls back to HOME dot-config
- **WHEN** `XDG_CONFIG_HOME` is unset or empty, `HOME=/home/user`, no `--config` or `BARDUCK_CONFIG` is given, and `/home/user/.config/barduck/config.toml` exists
- **THEN** startup reads `/home/user/.config/barduck/config.toml`

#### Scenario: Missing XDG default falls back to working directory
- **WHEN** no `--config` or `BARDUCK_CONFIG` is given, the XDG default file does not exist, and `./config.toml` exists
- **THEN** startup reads `./config.toml`

#### Scenario: Explicit config flag wins
- **WHEN** `--config ./config.toml` is passed while `BARDUCK_CONFIG` is set and an XDG default file exists
- **THEN** startup reads `./config.toml`, ignoring the env and XDG paths

#### Scenario: Config env var beats XDG default
- **WHEN** `BARDUCK_CONFIG=/srv/shared.toml` is set (non-empty), no `--config` is passed, and an XDG default file exists
- **THEN** startup reads `/srv/shared.toml`, not the XDG default

#### Scenario: Missing everywhere fails loudly
- **WHEN** no `--config` or `BARDUCK_CONFIG` is given and neither the XDG default file nor `./config.toml` exists
- **THEN** startup fails with an error naming the paths tried

#### Scenario: Used config path is reported
- **WHEN** startup resolves a config file successfully
- **THEN** the used path is printed to stderr
