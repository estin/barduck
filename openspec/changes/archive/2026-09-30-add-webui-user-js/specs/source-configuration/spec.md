# Spec Delta

## ADDED Requirements

### Requirement: User-defined web UI scripts configuration
The config file SHALL accept `web_user_js`: a list of paths, defaulting to empty. Each entry MAY be a concrete `.js` file or a directory; a directory entry SHALL expand to that directory's `*.js` files in alphabetic (byte-wise filename) order, non-recursive. Overall resolution order SHALL be config-list order, then alphabetic within each directory. Relative paths SHALL resolve against the directory containing the config file. Resolution SHALL happen once at startup; a configured path that is missing or unreadable SHALL produce a warning naming the path and be skipped, without failing startup. An empty directory SHALL contribute no scripts. A file resolving more than once (listed twice, or via overlapping entries) SHALL be injected only once. Like `sources` and `layouts`, this list SHALL NOT be overridable via environment variables.

#### Scenario: File and directory entries resolve in order
- **WHEN** `web_user_js = ["a.js", "extra/"]` with `extra/` containing `b.js` and `a.js`
- **THEN** scripts inject in the order `a.js`, `extra/a.js`, `extra/b.js`

#### Scenario: Missing path warns and skips
- **WHEN** `web_user_js` lists a path that does not exist or cannot be read
- **THEN** startup succeeds, logs a warning naming the path, and the page contains no tag for it

#### Scenario: Default is empty
- **WHEN** the config file omits `web_user_js`
- **THEN** startup succeeds and pages contain no injected scripts

#### Scenario: Relative paths resolve against the config directory
- **WHEN** the config file at `/etc/barduck/config.toml` sets `web_user_js = ["scripts/x.js"]`
- **THEN** the daemon resolves `/etc/barduck/scripts/x.js`

#### Scenario: Unknown neighboring keys still rejected
- **WHEN** the config file contains a misspelled unknown top-level key alongside a valid `web_user_js`
- **THEN** startup fails with a load error naming the unknown key
