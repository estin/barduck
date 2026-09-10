# barduck Skill Reference

> barduck is a single-binary home dashboard that collects values from config-defined shell sources, stores them in DuckDB, and displays them via a web UI, TUI, CLI, and JSON API.
> The config file lives at `~/.config/barduck/config.toml`. The app changes its working directory to the config file's directory at startup, so script commands resolve relative to it.

## Creating Sources by User Query

When a user asks to add a data source, translate their request into a TOML `[[sources]]` table. barduck supports two source types:

### Source Types

**`query`** — A oneshot shell command run on a schedule (`interval` or `cron`). Use this for periodic data points like disk usage, domain expiry, or weather.

**`stream`** — A long-running command emitting JSON lines to stdout. Use this for continuous data feeds like live API streams or log ingestion.

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

**Query-only fields:**

| Field | Required | Description |
|-------|----------|-------------|
| `interval` or `cron` | Yes | Schedule (mutually exclusive) |
| `history_points` | No | Override web UI history point count |

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

Validate a source configuration before applying it using `barduck fetch --source <name>` to run the command once without writing to the database. Check that:
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
2. **Choose the source type**: `query` for scheduled oneshot commands, `stream` for continuous JSON lines
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
| `barduck fetch` | Run one source once (debug) | `--source`, `--json` |
| `barduck skill` | Print this skill document | — |

### Query Modes

- **Direct DB** (default): Reads from the DuckDB file directly
- **Daemon mode**: Routes through the running daemon's HTTP API with `--daemon`

### Agent Workflow

1. **Query latest values**: `barduck latest` to see current readings
2. **Debug a source**: `barduck fetch --source <name>` to test a command
3. **Check logs**: `barduck logs --source <name>` to view fetch history
4. **Use `--json`** for machine-readable output in scripts

## System Architecture

- **Storage**: Embedded DuckDB (one file, plain SQL accessible)
- **Config**: TOML (`sources`, `layouts`) at `~/.config/barduck/config.toml` — adding a data point needs no code changes
- **Surfaces**: `daemon` (collector + API + web), `tui` (ratatui), CLI commands, all with direct-DB or daemon-backed modes
- **Collection**: `query` sources run on schedule; `stream` sources run continuously
- **Working directory**: barduck chdirs to the config file's directory at startup, so all script paths are relative to it
