# source-configuration Specification

## Purpose

Lets users declare where dashboard data comes from. Sources are defined declaratively in a config file using generic types, so adding a new data point requires no code changes.

## Requirements

### Requirement: Config file defines sources
The system SHALL read all source definitions from a single user config file at startup. Each source MUST have a unique name, a type, a schedule, and type-specific parameters.

#### Scenario: Valid config loads
- **WHEN** the config file contains one `http` source and one `script` source with valid parameters
- **THEN** both sources are registered and eligible for collection

#### Scenario: Invalid source rejected
- **WHEN** a source definition is missing required fields or has an unknown type or duplicate name
- **THEN** startup fails with an error naming the offending source and field

### Requirement: Environment variables override top-level settings
Each top-level `Config` scalar setting — `database_path`, `listen`, `interval`, `failure_threshold`, `history_points`, `tui_width` — MAY be overridden at startup by an environment variable named `BARDUCK_<FIELD>` (the field name upper-cased, e.g. `BARDUCK_LISTEN`, `BARDUCK_HISTORY_POINTS`). When such a variable is set (including to an empty string), its value SHALL be parsed using the same rules as the field's TOML representation (e.g. humantime for `interval`, integer for `history_points`/`failure_threshold`, `"auto"` or an integer for `tui_width`) and SHALL take precedence over both the config file's value for that field and the field's built-in default. A value that fails to parse MUST cause startup to fail with an error naming the environment variable and the parse failure. This override applies only to these top-level scalar fields, not to `sources` or `layouts`.

#### Scenario: Environment variable overrides config file value
- **WHEN** the config file sets `listen = "127.0.0.1:8420"` and `BARDUCK_LISTEN=0.0.0.0:9000` is set in the environment
- **THEN** the daemon binds to `0.0.0.0:9000`

#### Scenario: Environment variable overrides the built-in default
- **WHEN** the config file does not set `history_points` and `BARDUCK_HISTORY_POINTS=100` is set in the environment
- **THEN** the effective `history_points` is `100`

#### Scenario: No environment variable leaves the config file value in effect
- **WHEN** the config file sets `failure_threshold = 5` and no `BARDUCK_FAILURE_THRESHOLD` variable is set
- **THEN** the effective `failure_threshold` is `5`

#### Scenario: Unparseable override rejected
- **WHEN** `BARDUCK_FAILURE_THRESHOLD=not-a-number` is set in the environment
- **THEN** startup fails with an error naming `BARDUCK_FAILURE_THRESHOLD` and the parse failure

### Requirement: Generic http source type
The system SHALL support sources of type `http` that fetch a URL on schedule and extract a value via JSON path or similar selector.

#### Scenario: HTTP fetch produces a value
- **WHEN** an `http` source is configured with a URL returning `{"balance": 123.45}` and selector `balance`
- **THEN** a reading of `123.45` is produced for that source

#### Scenario: HTTP failure produces no value
- **WHEN** the URL is unreachable or returns a non-2xx status
- **THEN** no reading is recorded and the attempt is recorded in the fetch log as failed

### Requirement: Generic script source type
The system SHALL support sources of type `script` that run a local shell command on schedule and take its stdout as the value.

#### Scenario: Command output captured
- **WHEN** a `script` source runs `df / --output=pcent | tail -1` and it prints `42%`
- **THEN** a reading of `42%` is recorded for that source

#### Scenario: Failing command logged
- **WHEN** the command exits non-zero
- **THEN** no reading is recorded and the fetch log entry contains the exit status and stderr

### Requirement: Extensible source types
Adding a new source type MUST require only implementing the type's fetch behavior and registering it; source declarations referencing unregistered types MUST be rejected at startup.

#### Scenario: Unknown type named in error
- **WHEN** a source declares `type = "snmp"` which is not registered
- **THEN** startup fails with an error listing `snmp` as unknown

### Requirement: Sources may declare a display title
A source MAY declare an optional `title` field: a friendlier display label shown wherever the source's raw id would otherwise be used as a panel title or a group row's label. The system SHALL fall back to the source's id when no `title` is declared, matching prior behavior. An explicit per-cell override (a `{ id, title }` cell's `title`, or a group `ids` entry's `{ id, label }` `label`) SHALL always win over a source's own `title`.

#### Scenario: Source title used as default panel title
- **WHEN** a source `vds-base1` declares `title = "Base 1"` and a layout cell references it as the bare string `"vds-base1"` (no cell-level title override)
- **THEN** both UIs render that panel titled "Base 1"

#### Scenario: Source title used as a group's bare-id fallback label
- **WHEN** a group cell's `ids` contains the bare string `"vds-base1"` and that source declares `title = "Base 1"`
- **THEN** that value's row is labeled "Base 1"

#### Scenario: Explicit cell override still wins
- **WHEN** a source declares `title = "Base 1"` but the layout cell referencing it is `{ id = "vds-base1", title = "Custom" }`
- **THEN** the panel is titled "Custom", not "Base 1"

#### Scenario: No title falls back to the source id
- **WHEN** a source declares no `title`
- **THEN** its panel title (or group fallback label) is its raw id, as before this change

### Requirement: UI layouts are config-declared like sources
The system SHALL let users declare TUI and web dashboard layouts in the same config file as a row/column grid: each layout has a `title` and `rows`, where a row is a list of cells and a cell is one of
- a string naming a source (shorthand), or
- a table `{ id = "<source>", title = "<pane title>" }` overriding the default pane title (the source's declared `title`, or its name if it has none), or
- a table `{ kind = "space", colspan = <columns> }` rendering an empty spacer, or
- a table `{ title?, format?, text }` rendering a standalone static-text panel with no backing source: `text` is the literal content (required), `format` is optional (`text` default, `markdown`, or `json`, same values as a source's `format`), and `title` is an optional pane header (omitted renders no header text, since there's no source to fall back to a label from), or
- a table `{ title?, main?, secondary?, table? }` rendering one generalized pane, where `title` is an optional pane header, `main` is an optional single member, and `secondary` and `table` are each an optional list of members. A "member" is either a bare source-id string (its label defaults to that source's declared `title`, or its id if it has none) or a table `{ id = "<source>", label = "<value label>" }` overriding the label. At least one of `main`, `secondary`, or `table` MUST be present; `title` MAY be omitted entirely — the rendered pane then falls back to `main`'s own label when `main` is set, else shows no header text.

The layout's column count SHALL be the maximum number of columns spanned by any row; shorter rows are implicitly padded. Validation MUST reject rows referencing unknown sources (including any source id inside a generalized pane cell's `main`, `secondary`, or `table`), cells that are neither a valid source reference, space, static-text panel, nor generalized pane, `space` cells without `colspan`, colspans smaller than 1, static-text panel cells with an empty `text`, generalized pane cells with an explicitly empty `title` (`title = ""`), and generalized pane cells with `main`, `secondary`, and `table` all absent. The legacy flat `sources = [...]` layout key is removed (**BREAKING**). The prior `{ title, ids = [...] }` shape is replaced by `table` (**BREAKING**): existing configs must rename `ids` to `table`.

#### Scenario: Layout references existing source
- **WHEN** a layout row contains cell `"domain-expiry"`
- **THEN** the TUI and web dashboards render that source's latest value in that position

#### Scenario: Custom pane title
- **WHEN** a cell is `{ id = "status-json", title = "Status" }`
- **THEN** both UIs render the pane titled "Status"

#### Scenario: Spacer spans columns
- **WHEN** a row is `[{ kind = "space", colspan = 2 }, "work-hours"]` in a 3-column layout
- **THEN** the first two column slots are empty and `work-hours` occupies the third

#### Scenario: Unknown source in grid rejected
- **WHEN** a layout row references source `nope`
- **THEN** startup fails naming the layout and the unknown source

#### Scenario: Group pane with labeled values
- **WHEN** a cell is `{ title = "ihor", table = [{ id = "vds-base1", label = "days left" }, { id = "vds-base1-balance", label = "balance" }] }`
- **THEN** both UIs render one pane titled "ihor" containing both values as table rows, each shown under its configured label

#### Scenario: Group pane with a bare id falls back to the id as its label
- **WHEN** a generalized pane cell's `table` contains the bare string `"vds-base1"` (no `label`) and that source declares no `title`
- **THEN** that value's row is labeled `"vds-base1"`

#### Scenario: Unknown source inside a group rejected
- **WHEN** a generalized pane cell's `main`, `secondary`, or `table` references source `nope`
- **THEN** startup fails naming the layout and the unknown source

#### Scenario: Group cell with empty ids rejected
- **WHEN** a generalized pane cell declares none of `main`, `secondary`, or `table`
- **THEN** startup fails naming the layout and the empty pane

#### Scenario: Pane combines main, secondary, and table sections
- **WHEN** a cell is `{ title = "Server", main = "cpu-load", secondary = ["mem-used", "disk-free"], table = [{ id = "vds-base1", label = "days left" }] }`
- **THEN** both UIs render one pane titled "Server" containing `cpu-load` as the main section, `mem-used` and `disk-free` as secondary members, and `vds-base1` as a table row, all inside the same pane

#### Scenario: Pane with only a main section
- **WHEN** a cell is `{ title = "CPU", main = "cpu-load" }`
- **THEN** both UIs render one pane titled "CPU" containing only `cpu-load`'s main-section rendering

#### Scenario: Title omitted falls back to main's own label
- **WHEN** a cell is `{ main = "cpu-load" }` with no `title`
- **THEN** both UIs render the pane titled with `cpu-load`'s own declared `title` (or its id if it has none), the same as a bare `"cpu-load"` cell

#### Scenario: Title omitted with no main renders no header text
- **WHEN** a cell is `{ secondary = ["disk-home", "disk-root"] }` with no `title` and no `main`
- **THEN** both UIs render the pane with no header text, containing the two secondary members

#### Scenario: Explicitly empty title rejected
- **WHEN** a generalized pane cell is `{ title = "", main = "cpu-load" }`
- **THEN** startup fails naming the layout and the empty title

#### Scenario: Static-text panel renders standalone content
- **WHEN** a cell is `{ title = "Links", format = "markdown", text = "- [GitHub](https://github.com)" }`
- **THEN** both UIs render one panel titled "Links" containing that content, with no backing source

#### Scenario: Static-text panel with no title renders no header text
- **WHEN** a cell is `{ text = "Just a note." }` with no `title`
- **THEN** both UIs render the panel with no header text, containing the note

#### Scenario: Static-text panel defaults to plain text format
- **WHEN** a cell is `{ text = "Plain note" }` with no `format`
- **THEN** both UIs render the content as plain text, the same default as a source with no declared `format`

#### Scenario: Empty static-text content rejected
- **WHEN** a cell is `{ title = "Links", text = "" }`
- **THEN** startup fails naming the layout and the empty text panel

### Requirement: Value formats
A source MAY declare a `format` of `text` (default), `markdown`, or `json`; other values MUST be rejected at startup. UIs SHALL render markdown as HTML and pretty-print valid JSON.

#### Scenario: Unknown format rejected
- **WHEN** a source declares `format = "rst"`
- **THEN** startup fails naming the unknown format

#### Scenario: Markdown rendered as markup
- **WHEN** a markdown-format source yields a reading containing `# Heading`
- **THEN** the web UI renders it as an HTML heading

### Requirement: Threshold bands
A source MAY declare threshold bands as `{bound, level}` pairs where `level` is `green`, `yellow`, or `red`. The reading's numeric value selects the first band whose bound is greater or equal (bands sorted by bound); levels may be ordered green→red or red→green. Non-numeric readings get no band color. Invalid levels or a single-band list MUST be rejected at startup.

#### Scenario: High value turns red
- **WHEN** a source has bands 60→green, 85→yellow, 100→red and reports `92`
- **THEN** its panel renders with the red band style

### Requirement: Source setup commands
A source MAY declare a `setup` field containing a shell command run by the collector before that source's first fetch (for example to start a service or open a tunnel). The command runs under the same shell semantics as `script` sources and honors the source's timeout.

#### Scenario: Setup declared and validated
- **WHEN** a config declares `setup = "systemctl start my-exporter"` on a source
- **THEN** the source loads normally and the command runs before its first fetch

#### Scenario: Setup applies to any source type
- **WHEN** either an `http` or a `script` source declares a `setup` command
- **THEN** both gate their fetching behind it

### Requirement: Config-relative working directory
At startup, before loading the config file or running any command, the system SHALL resolve the config file path to an absolute path and change its working directory to that file's parent directory. Every relative path inside the config file (such as `database_path`) and every command the system executes (a `script` source's `command`, a source's `setup` command) SHALL be resolved relative to the config file's directory, not the directory the process was launched from.

#### Scenario: Relative database path resolves against config directory
- **WHEN** `database_path` is a relative path and the config file lives in `/srv/dashboard/config.toml`
- **THEN** the database file is opened at `/srv/dashboard/<database_path>` regardless of the process's launch directory

#### Scenario: Source commands resolve against config directory
- **WHEN** a `script` source's `command` or a source's `setup` command references a relative script path
- **THEN** the command runs with its working directory set to the config file's directory, so the relative path resolves there

#### Scenario: Same config runs identically from any launch directory
- **WHEN** the same config file is launched via `--config /srv/dashboard/config.toml` from two different starting directories
- **THEN** both runs resolve all relative config paths and commands identically

### Requirement: Configurable history bar depth
The system SHALL support a global default and an optional per-source override, `history_points`, controlling how many recent readings a source's web UI history bar shows. The global default MUST be used for any source that does not declare its own `history_points`. `history_points` MUST be a positive integer; a non-positive or non-integer value MUST be rejected at startup.

#### Scenario: Default applies when unset
- **WHEN** a threshold-banded source declares no `history_points`
- **THEN** its history bar shows the global default number of recent readings

#### Scenario: Per-source override honored
- **WHEN** a source declares `history_points = 50` and the global default is 30
- **THEN** that source's history bar shows up to 50 recent readings

#### Scenario: Invalid value rejected
- **WHEN** a source declares `history_points = 0`
- **THEN** startup fails naming the source and the invalid value

### Requirement: Per-source history bar visibility
A source MAY declare an optional `show_history` boolean field controlling whether its web UI history bar renders. The system SHALL default it to `true` (unchanged behavior: a threshold-banded source shows its history bar). Setting `show_history = false` MUST hide that source's history bar even though it declares threshold bands. `show_history` SHALL have no effect on a source with no threshold bands, since those never render a history bar regardless (spec: web-ui — panel retrospective history bar).

#### Scenario: Default shows the bar
- **WHEN** a threshold-banded source declares no `show_history` field
- **THEN** its history bar renders as before this change

#### Scenario: Explicit false hides the bar
- **WHEN** a threshold-banded source declares `show_history = false`
- **THEN** its panel renders with no history bar

#### Scenario: No effect on an unbanded source
- **WHEN** a source with no threshold bands declares `show_history = true`
- **THEN** its panel still renders no history bar, since it has no bands to derive segments from

### Requirement: Per-source view visibility
A source MAY declare a `show_in` field controlling which UI(s) are allowed to display it: `"all"` (default, unchanged behavior), `"tui"` (visible only in the TUI), or `"web"` (visible only in the web dashboard). Any other value MUST be rejected at startup, naming the source and the invalid value. `show_in` SHALL have no effect on data collection: the source is fetched on its configured schedule regardless of its value.

#### Scenario: Default is visible everywhere
- **WHEN** a source declares no `show_in` field
- **THEN** it is eligible to display in both the TUI and the web dashboard, as before this change

#### Scenario: Restricting to one view hides it from the other
- **WHEN** a source declares `show_in = "tui"` and is referenced by a layout cell
- **THEN** the source's panel appears in the TUI but not on the web dashboard, even though both UIs share the same layout config

#### Scenario: Invalid value rejected
- **WHEN** a source declares `show_in = "cli"`
- **THEN** startup fails naming the source and the invalid value

#### Scenario: Collection unaffected by view restriction
- **WHEN** a source declares `show_in = "web"`
- **THEN** the source is still fetched on its configured schedule, even though it never displays in the TUI

### Requirement: Per-source fetch retry interval
A source MAY declare a `retry_interval` field (humantime duration) controlling how soon an interval-scheduled source retries after a failed fetch, instead of waiting the full `interval`. It defaults to `30s` when not declared. `retry_interval` has no effect on a source's setup-command retries (those always wait the normal schedule tick) and no effect on a cron-scheduled source. A source MUST NOT declare both `cron` and `retry_interval`; declaring one with the other MUST be rejected at startup, naming the source.

#### Scenario: Default retry interval applies without configuration
- **WHEN** a source declares no `retry_interval`
- **THEN** it retries 30 seconds after a failed fetch, without any config change

#### Scenario: Explicit retry interval overrides the default
- **WHEN** a source declares `retry_interval = "10s"`
- **THEN** it retries 10 seconds after a failed fetch

#### Scenario: Retry interval with cron rejected
- **WHEN** a source declares both `cron = "0 */5 * * * *"` and `retry_interval = "10s"`
- **THEN** startup fails naming the source and stating that `retry_interval` has no effect on a cron-scheduled source

### Requirement: Human-readable duration configuration
Every duration-valued config field — a source's `interval`, `retry_interval`, and `timeout`, and the top-level default `interval` — SHALL be a humantime-formatted string (e.g. `"30s"`, `"5m"`, `"1h30m"`, `"2d"`), not a raw integer of seconds. A field that is not a valid humantime duration string (malformed text, or a nonzero bare number with no unit) MUST be rejected at startup with an error naming the offending source (or the top-level field) and the invalid value. An unrecognized field name on a source or at the top level (for example a pre-rename `interval_secs`, or a leftover `stale_after`) MUST also be rejected at startup rather than silently ignored, so a config left over from before this change fails loudly instead of quietly reverting to a default.

#### Scenario: Humantime string accepted
- **WHEN** a source declares `interval = "5m"`
- **THEN** the source is fetched every 5 minutes

#### Scenario: Invalid duration string rejected
- **WHEN** a source declares `timeout = "banana"`
- **THEN** startup fails naming the source, the field, and the invalid value

#### Scenario: Bare number without a unit rejected
- **WHEN** a source declares `interval = "300"`
- **THEN** startup fails naming the source, the field, and the invalid value

#### Scenario: Leftover pre-rename field rejected
- **WHEN** a config written before this change still declares `interval_secs = 300` on a source
- **THEN** startup fails naming the source and the unrecognized field, instead of silently falling back to the default interval

#### Scenario: Leftover top-level stale_after rejected
- **WHEN** a config written before this change still declares a top-level `stale_after = "30m"`
- **THEN** startup fails naming the unrecognized field, instead of silently ignoring it

### Requirement: Cron schedule
A source MAY declare a `cron` field containing a cron expression (evaluated with the `croner` crate; standard cron syntax with an optional leading seconds field, e.g. `"0 0 3 * * *"` for daily at 03:00) instead of `interval`. A source MUST NOT declare both `cron` and `interval`, nor both `cron` and `retry_interval` (spec: source-configuration — Per-source fetch retry interval). The system SHALL validate the cron expression at startup and reject an invalid expression, naming the source and the invalid value.

#### Scenario: Cron schedule accepted
- **WHEN** a source declares `cron = "0 0 3 * * *"` and no `interval`
- **THEN** the source is scheduled to fetch at that cron expression's occurrences instead of a fixed interval

#### Scenario: Both interval and cron rejected
- **WHEN** a source declares both `interval = "5m"` and `cron = "0 */5 * * * *"`
- **THEN** startup fails naming the source and stating that only one of `interval`/`cron` may be set

#### Scenario: Invalid cron expression rejected
- **WHEN** a source declares `cron = "not a cron"`
- **THEN** startup fails naming the source and the invalid cron expression
