# barduck Skill Reference

> barduck is a single-binary home dashboard that collects values from config-defined shell sources, stores them in DuckDB, and displays them via a web UI, TUI, CLI, and JSON API.
> Pass the TOML config with `--config` (default: `config.toml`). Each source command runs in the config file's directory, so script paths resolve relative to it without changing the application's working directory.

## Creating Sources by User Query

When a user asks to add a data source, translate their request into a TOML `[[sources]]` table. barduck supports three source types:

### Source Types

**`query`** — A oneshot shell command run on a schedule (`interval` or `cron`). Use this for periodic data points like disk usage, domain expiry, or weather.

**`stream`** — A long-running command emitting JSON lines to stdout. Use this for continuous data feeds like live API streams or log ingestion.

**`ingest`** — A push-based source that receives data via HTTP POST. Use this for webhooks, event buses, or any producer that pushes values to barduck's `/api/ingest` endpoint. Has no `command`; data arrives via push. Reports `stale` when no push arrives within `expected_interval`.

### Platform shell semantics

`command` and `setup` execute through `sh -c` on Linux/macOS and
`cmd.exe /D /S /C` on native Windows. Use the host shell's quoting,
environment-variable syntax, and installed tools. POSIX examples below
need adaptation on Windows; single quotes are not `cmd.exe` string quotes.
To use PowerShell, invoke it explicitly, for example:

```toml
command = 'powershell.exe -NoProfile -File "scripts/metric.ps1"'
```

Timeout/cancellation and stream shutdown terminate descendants through
Unix process groups or Windows Job Objects, not just the shell PID.
Background processes belong to the source's lifetime; do not deliberately
detach them from their Unix process group. Emit UTF-8 output from scripts.

### Composite Sources

A `query` source may declare `children`, turning it into a **composite source**: one command run that reports several independently-displayed, independently-healthed values instead of one. Use this when a single cheap command naturally produces a small family of related numbers — load averages for 1/5/15 minutes, per-partition disk usage, multi-sensor readings — instead of writing one `[[sources]]` entry (and spawning one process) per number.

```toml
[[sources]]
name = "load"
type = "query"
command = "sh scripts/load-average.sh"
interval = "10s"

[[sources.children]]
name = "1m"
unit = "avg"

[[sources.children]]
name = "5m"
unit = "avg"

[[sources.children]]
name = "15m"
unit = "avg"
thresholds = [
  { bound = 2.0, level = "green" },
  { bound = 4.0, level = "yellow" },
  { bound = 8.0, level = "red" },
]
```

Each child becomes an ordinary, independently addressable source named `<parent>::<child>` (here `load::1m`, `load::5m`, `load::15m`) with its own `title`, `unit`, `format`, `thresholds`, `show_history`, `show_in`, `value_type`, and `history_points` — but **no** `command`, `interval`/`cron`, `timeout`, `setup`, or `retry_interval` of its own: those stay on the parent, which runs on its own schedule exactly like any other `query` source.

**Output contract:** instead of a single value, the composite root's command must print a JSON array of items shaped like an `/api/ingest` payload — `source` (the child's full name), `value`, and optionally `ts` and `thresholds`:

```json
[
  {"source": "load::1m", "value": "0.42"},
  {"source": "load::5m", "value": "0.38"},
  {"source": "load::15m", "value": "0.31"}
]
```

Each entry is stored to its named child exactly as an equivalent `/api/ingest` push would be. The root itself never stores a scalar reading — its own fetch log only reflects whether the command ran and its output parsed as the array above. A declared child missing from the array fails just that child (logged as its own failed attempt); an entry naming an id that isn't a declared child fails the whole attempt for that tick, since it signals a misconfigured or drifted script.

**Validation:** the composite root's own `unit`, `thresholds`, `value_type`, `format`, and `show_history` are rejected at config load — they'd never apply to a value the root itself doesn't produce. `title`, `show_in`, and the root's own schedule fields remain valid. `children` must be non-empty when declared; a bare child name must be unique among its siblings and must not contain `::`. Composite sources are `query`-only — `stream` and `ingest` sources cannot declare `children`, and a child cannot itself have children (no nesting).

**Force polling and display:** forcing a poll of the root or of any one child runs the parent's command once and refreshes every declared child from that single run (`poll --source load` and `poll --source load::1m` are equivalent in what they trigger, differing only in whose outcome is reported back). A layout cell that references the composite root's own name (not a child) auto-renders as a table listing every child's current value, in declared order — the same shape a manually-authored `{ table = [...] }` cell already produces — so referencing `"load"` directly shows all three averages without listing them out. Children remain individually referenceable in any layout cell too.

### Configuration Format

Sources are defined in `config.toml` using `[[sources]]` tables. Each source is deserialized through a per-type enum — cross-type fields fail at parse time.

```toml
[[sources]]
name = "cpu_temp"
type = "query"
command = "sensors | grep Package | awk '{print $4}'"
interval = "10s"
timeout = "5s"
unit = "°C"

[[sources]]
name = "price_feed"
type = "stream"
command = "nc localhost 9090"
expected_interval = "1s"
timeout = "30s"
```

```toml
[[sources]]
name = "webhook"
type = "ingest"
expected_interval = "1m"
unit = "events"
format = "json"
```

### Key Fields

**Common fields** (both `query` and `stream`):

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique source identifier |
| `title` | No | Display label shown in the UI |
| `type` | Yes | `query` or `stream` |
| `command` | Yes | Shell command to execute |
| `timeout` | Yes | Command timeout (humantime format) |
| `interval` | query only | Schedule interval (`"10s"`, `"1m"`, `"1h"`). Mutually exclusive with `cron`. |
| `cron` | query only | Cron expression (e.g. `"0 0 3 * * *"`). Mutually exclusive with `interval`. |
| `unit` | No | Display unit (e.g. `"days"`, `"%"`, `"RUB"`) |
| `format` | No | Rendering format: `text`, `markdown`, or `json` |
| `thresholds` | No | Color bands: `[{ bound = 60.0, level = "green" }, { bound = 85.0, level = "yellow" }]` |
| `show_history` | No | Whether to show history bar in web UI (default `true`) |
| `show_in` | No | Which UI surfaces display this source: `all` (default), `tui`, or `web` |
| `value_type` | No | Storage type: `string` (default), `bigint`, `double`, or `json` |
| `setup` | No | Pre-fetch shell command to run first (e.g., start a service) |
| `retry_interval` | No | Retry interval on failure |

**Stream-only fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `expected_interval` | Yes | Maximum silence between values before reporting stale |
| `retry_interval` | No | Delay before reopening after command exits |

**Ingest-only fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `expected_interval` | Yes | Maximum silence between pushes before reporting stale |

**Query-only fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `interval` or `cron` | Yes | Schedule (mutually exclusive) |
| `history_points` | No | Override web UI history point count |
| `children` | No | Declares this a composite source (see [Composite Sources](#composite-sources)); each `[[sources.children]]` entry takes `name` plus a child's own `title`/`unit`/`format`/`thresholds`/`history_points`/`show_history`/`show_in`/`value_type` |

### Value Types

- `string` — Default. Stored as-is.
- `bigint` — Also populates a `bigint` column for numeric queries.
- `double` — Also populates a `double` column.
- `json` — Also populates a `json` column. Invalid JSON surfaces through the standard error path.

### Threshold Bands

Thresholds map numeric values to color levels (green/yellow/red). A lone band is rejected — at least two are required. Bands are interpreted ascending by bound.

```toml
thresholds = [
  { bound = 80.0, level = "green" },
  { bound = 95.0, level = "yellow" },
  { bound = 100.0, level = "red" },
]
```

### Validation

Validate a source configuration before applying it using `barduck fetch --source <name>` to run the command once without writing to the database (`barduck poll --source <name>` runs it and *does* write). Check that:
- The command executes successfully
- Output parses correctly for the declared `value_type`
- `thresholds` bands are valid (at least two bands for numeric sources)

### Best Practices

**Do not inject user secrets into commands.** Commands run in barduck's process and the config file may contain sensitive values. Instead:

- Use `stream` sources to pipe values from trusted sources or HTTP ingest APIs
- If authentication is needed, use environment variables or scripts that read secrets from secure stores outside the config
- Never put API keys, passwords, or tokens directly in `command` fields

**Scripts resolve relative to the config directory.** barduck changes its working directory to the config file's directory at startup, so scripts referenced in `command` can use relative paths. For example, if `config.toml` is at `~/.config/barduck/config.toml` and `scripts/` is in the same directory, use `command = "nu scripts/metric.nu"` (not an absolute path).

### Agent Workflow

1. **Parse the user's request** to identify the data point and source type
2. **Choose the source type**: `query` for scheduled oneshot commands, `stream` for continuous JSON lines, `ingest` for push-based data arriving via HTTP POST
3. **Write the TOML source configuration** with the appropriate fields, using `name` (not `id`)
4. **Validate** by running `barduck fetch --source <name>` to test without affecting the database
5. **Add to config.toml** and restart the daemon (or use the running daemon if supported)

## Layout Configuration

Layouts organize sources into visible panels in the dashboard.

### Layout Structure

```toml
[[layouts]]
title = "System"
rows = [
  [
    { main = "load-average" },
    { table = ["disk-root", "disk-home"] },
    { main = "vpn-status" },
  ],
]
```

- `[[layouts]]` — Each layout has a `title` and a `rows` field
- `rows` — A 2D array (rows × columns) of `Cell` values
- `Cell` can be:
  - Bare string: `"load-average"` — simple source reference
  - `{ main = "src" }` — source in the main column
  - `{ table = ["src1", "src2"] }` — sources in a table view
  - `{ main = "src", secondary = ["other"], title = "Label" }` — named group with secondary sources
  - `{ kind = "space", colspan = 2 }` — empty spacer cell spanning 2 columns
- Referencing a [composite source](#composite-sources)'s own name (bare, or as a cell's `main`) auto-renders a table of its children — no need to list them out with `table = [...]`; its children remain individually referenceable too
### Web UI Style Overrides

Layouts and cells can have a `style` field with CSS property overrides rendered as inline styles in the web dashboard.

```toml
[[layouts]]
title = "Monospace Dashboard"
style = { font_family = "monospace", font_size = "14px" }
rows = [
  [{ main = "cpu-temp", style = { font_size = "18px" } }],
  [{ main = "load-average", style = { font_family = "sans-serif" } }],
]
```

- `style` on `[[layouts]]` applies CSS overrides to the entire layout grid container
- `style` on `Cell` objects (`main`, `table`, `Pane`, `Text`, `Group`) applies per-panel CSS overrides
- Any CSS property-value pair is supported (e.g., `font_family`, `font_size`, `background_color`, `gap`)

### Visibility


Each source has a `show_in` field controlling which UI surfaces display it:
- `show_in = "all"` (default) — Visible in both TUI and web UI
- `show_in = "tui"` — Only visible in the TUI
- `show_in = "web"` — Only visible in the web UI

Sources not visible in a surface render as empty space rather than failing.

### Agent Workflow

1. **Identify which sources** the user wants to see together
2. **Define a layout** with `[[layouts]]`, `title`, and `rows` using source `name` values
3. **Use `Cell` objects** (`main`, `table`, `secondary`) to arrange sources in panels
4. **Set `show_in`** to control which surfaces see the source

## CLI Commands

All commands support a `--config` flag (default: `config.toml`).

| Command | Description | Key Flags |
|---------|-------------|-----------|
| `barduck daemon` | Run collector + HTTP API + web UI | `--config` |
| `barduck tui` | Terminal dashboard (ratatui) | `--daemon` |
| `barduck latest` | Latest value per source | `--json`, `--source`, `--no-text` |
| `barduck logs` | Recent fetch logs | `--limit`, `--json`, `--source` |
| `barduck reset` | Permanently delete all data | `--yes`, `--json` |
| `barduck poll` | Fetch sources now and store the results | `--source` (repeatable, required), `--json`, `--daemon` |
| `barduck fetch` | Run one source once, writing nothing (debug) | `--source`, `--json` |
| `barduck skill` | Print this skill document | — |

### `poll` vs `fetch`

Both run a source's command once, off-schedule. They differ in what they
leave behind:

- `barduck poll --source <name>` **writes**: a reading, a fetch-log entry
  with origin `poll`, a health refresh — exactly what a scheduled fetch
  records. Use it to refresh a stale panel now. Repeatable `--source`; exits
  non-zero if any attempt failed, after attempting all of them.
- `barduck fetch --source <name>` **writes nothing**: it prints the parsed
  result (extracted value, resolved timestamp, thresholds, type conversion)
  so a new source's command can be validated before it goes live.

`poll` needs no daemon: with none running it fetches in-process and writes to
the database file; with one running it routes through that daemon's API
automatically, since only one process can hold the database. In the web UI
the same action is the "poll now" control on each panel and log view.

While a source's fetch is actually running — scheduled or forced, triggered
from anywhere — both the web dashboard and the TUI show it as currently
polling (the web control becomes a plain "polling…" marker; the TUI panel
title/line gets a "(polling)" suffix), independent of that source's health
status. This is a live shared signal: every viewer sees it, not just whoever
triggered the poll, so there's no separate progress or failure popup — a
failed attempt shows up the normal way, through the source's health and
fetch log.

Both `poll` and `fetch` accept a [composite source](#composite-sources)'s own
name or one of its children's full `<parent>::<child>` name. Either way runs
the parent's command exactly once and refreshes every declared child from
that one run — `poll --source load` and `poll --source load::1m` trigger the
same fetch, differing only in whose outcome is printed; `fetch --source
load` prints every child's parsed entry, `fetch --source load::1m` just that
child's. All of {root, every child} show the shared polling marker for the
duration of that one command.

### Query Modes

- **Direct DB** (default): Reads from the DuckDB file directly
- **Daemon mode**: Routes through the running daemon's HTTP API with `--daemon`

### Agent Workflow

1. **Query latest values**: `barduck latest` to see current readings
2. **Debug a source**: `barduck fetch --source <name>` to test a command
   without writing; `barduck poll --source <name>` to fetch *and* store
3. **Check logs**: `barduck logs --source <name>` to view fetch history
4. **Use `--json`** for machine-readable output in scripts

## System Architecture

- **Storage**: Embedded DuckDB (one file, plain SQL accessible)
- **Config**: TOML (`sources`, `layouts`) selected with `--config` (default: `config.toml`) — adding a data point needs no code changes
- **Surfaces**: `daemon` (collector + API + web), `tui` (ratatui), CLI commands, all with direct-DB or daemon-backed modes
- **Collection**: `query` sources run on schedule; `stream` sources run continuously
- **Working directory**: each command runs in the config file's directory; the application's working directory is unchanged
