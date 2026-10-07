# barduck demo

A runnable demo of the dashboard: one Rust binary that collects values from
config-defined shell commands on a schedule (or continuously), stores them in
DuckDB, and shows them via web UI (live-updating), CLI, TUI, and a JSON HTTP API.

The demo config mirrors a real self-hosted dashboard (see the layout, source
and pane shapes below) while keeping every command harmless.

## Run it

Build from the repository root; once built, the daemon can be launched with
`--config` from anywhere — it resolves `database_path` and every source's
`command`/`setup` relative to the config file's own directory, not wherever
you happen to be.

```sh
# Build, bundle assets, and start the daemon (collector + web UI + HTTP API)
just run daemon --config demo/config.toml
```

`just run` always builds the release binary and bundles its assets together,
then runs `target/release/barduck` — the two have to come from the
same build (the Tailwind stylesheet asset's ID is derived from the build
script's `OUT_DIR`, which differs between builds), so don't `cargo install`
this app or mix a `target/release` binary with an asset bundle from a
different build; run `./target/release/barduck` directly instead.

DuckDB links against a system install by default (`brew install duckdb` /
`apt install libduckdb-dev` / etc.). Without one available, build with
`--features bundled` instead to compile DuckDB from source — `just run`
doesn't forward cargo flags, so do this step manually:

```sh
cargo build --release --features bundled
# If that fails to compile with an assembler assertion (binutils 2.47 bug):
#   CXXFLAGS="-g0 -w" cargo build --release --features bundled
topcoat asset bundle --release
./target/release/barduck daemon --config demo/config.toml
```

Then:

| What | Where |
|---|---|
| Web UI (live) | http://127.0.0.1:18420/ |
| Latest values | `barduck --config demo/config.toml latest` |
| Filter by source | append `-s load-averages -s disk-root` to `latest`/`logs` |
| Source fetch logs | click any panel's "updated …" text → `/logs/<source>` |
| Debug-fetch one source | `... fetch --source cloud-balance` (no database writes) |
| Fetch logs | `... logs --limit 10` |
| JSON output | add `--json` to `latest`/`logs`/`fetch`/`reset` |
| TUI | `... tui` (`q` quits) |
| Query via daemon instead of the DB file | add `--daemon` |
| Push an ingest value | `curl -X POST localhost:18420/api/ingest -d '{"source":"webhook-events","value":"7"}'` |

## What to watch

- **Layouts mirror real use** — three pages: "Host" (monospace, host-health
  pane with a `main` value, `secondary` values, and a `table` row), "Cloud &
  sites" (money + site + state demos), "Reports & misc" (markdown panes,
  generalized and composite sources).
- **Live web values** — panels update without a page reload (topcoat shard
  re-rendering server-side; `load-averages` and `cpu-temp` vary naturally,
  the rest re-fetch on their own schedules).
- **Composite sources** — one command, many panels. `load-averages` parses
  `uptime` and fans out to three children (1m/5m/15m), each independently
  displayed and healthed; `memory` does the same for `free -b` (mem/swap).
  Address one child as `load-averages::5m`; the bare name renders a table
  of all three (see "Reports & misc").
- **Generalized panes** — the "This host" pane combines three sections in
  one: `load-averages::5m` as `main` (large text, its own color, its own
  history bar), `memory::mem`/`memory::swap` as `secondary` (compact colored
  values, no bars), and `disk-home` as a `table` row labeled "home". The
  pane's border tracks the worst member across all three.
- **Threshold colors** — `disk-root` goes green → yellow → red as `/` fills;
  `cloud-balance` uses the opposite direction (more USD is greener).
- **JSONL row bands** — `cloud-usage` is a `query` source whose command
  prints a `jsonl` row carrying both the value and replacement threshold
  bands; the row's bands win over the declared ones.
- **Source titles** — `disk-root` declares `title = "Root filesystem"`; the
  bare-id `main` entry in "Reports & misc" would show that title; child
  titles like `load-5m` come from the child's own `title`.
- **History bar opt-out** — `disk-home` is threshold-banded but declares
  `show_history = false`, so its table row shows no bar; compare with
  `disk-root` in `main`, which is banded the same way and does show one.
  `memory::mem` in `secondary` shows no bar either — `secondary` never
  renders one.
- **Markdown values** — the "Weekly report" and "Status note" panels render
  markdown as HTML (headings, bold, task lists).
- **Static-text panels** — the "Quick Links" pane is a standalone
  `{ text = "..." }` layout cell, not a source: no `[[sources]]` entry, no
  schedule, no health, no log view, no summary-strip chip.
- **Per-view visibility** — `dead-service` declares `show_in = "tui"`: it
  shows in the TUI (troubleshooting) but not on the web dashboard.
  `public-status-note` does the opposite (`show_in = "web"`). Both are
  still fetched on schedule; `show_in` only controls display.
- **Failing source** — `dead-service` runs `exit 1`; after 2
  consecutive failures its panel turns red and `health` reports `failing`,
  with a plain "failing" label alongside the color even though it declares no
  threshold bands. Other sources keep collecting on schedule.
- **Setup commands** — `tunneled-service` has a setup command that fails until
  you run `touch /tmp/bd-tunnel-up`; watch it retry on its schedule, then turn
  healthy and start fetching without a daemon restart. `bank-balance`'s setup
  always succeeds; remove the file it checks for and restart to see a failing
  setup recover.
- **Config-relative commands** — `load-averages`, `memory` (and the rest)
  run scripts via relative paths; they resolve against `demo/` (this
  config file's directory) no matter where the daemon was launched from.
- **Stale stream** — `quiet-stream` emits a single value, then stays silent;
  it turns amber (`stale`) once silence exceeds its `expected_interval`
  (`"20s"`), while its process keeps running.
- **Ingest source** — `webhook-events` has no command at all; push values
  with the `curl` above and watch staleness governed by `expected_interval`.
- **Restart persistence** — Ctrl-C the daemon, restart it: history is still
  there.

## Storage

Everything lands in one DuckDB file, `demo/dashboard.duckdb`, Open it with any
DuckDB client while the daemon is stopped — or use the daemon's API:

```sh
duckdb demo/dashboard.duckdb \
  "SELECT source, value, ts FROM readings ORDER BY ts_epoch DESC LIMIT 5"
curl -s localhost:18420/api/sources/cloud-balance/history | head
```

Tables: `readings`, `fetch_logs`, `health_events`, `source_thresholds`.

## How it fits together

```
config.toml ──► sources (query / stream / ingest) ──► scheduler (tokio, per-source tasks)
                     │                                   │
                     │                                   ▼
                     │                  fetch_logs / readings / health_events
                     │                                   │
                     ▼                                   ▼
              topcoat web UI ◄──────────────────── DuckDB (single file)
              JSON API (/api/…)                          ▲
                     │                                   │ direct read-only
             CLI / TUI ──────────────────────────────────┘
                    (--daemon routes over the API)
```

- Sources are declared in TOML — `query` (an oneshot shell command per
  schedule tick, plain or `jsonl` stdout), `stream` (a long-running shell
  command emitting one `jsonl` row per line), and `ingest` (no command;
  values arrive via HTTP push) types; no code changes to add a data point.
- Layouts are config too: `[[layouts]]` lists source names per panel; TUI and
  web render the same definition.
- One binary, four surfaces. Planning docs live in `openspec/`.
