# barduck demo

A runnable demo of the dashboard: one Rust binary that collects values from
config-defined sources on a schedule, stores them in DuckDB, and shows them via
web UI (live-updating), CLI, TUI, and a JSON HTTP API.

## Run it

Build from the repository root; once built, the daemon can be launched with
`--config` from anywhere — it resolves `database_path` and every source's
`command`/`setup` relative to the config file's own directory, not wherever
you happen to be.

```sh
# 1. Start the mock upstream APIs
(cd demo && python3 mock_server.py) &

# 2. Build, bundle assets, and start the daemon (collector + web UI + HTTP API)
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
| Filter by source | append `-s bank-balance -s disk-root` to any query |
| Source fetch logs | click any panel's "updated …" text → `/logs/<source>` |
| History | `... history bank-balance` |
| Health | `... health` |
| Fetch logs | `... logs --limit 10` |
| JSON output | add `--json` to any query command |
| TUI | `... tui` (`q` quits) |
| Query via daemon instead of the DB file | add `--daemon` |

## What to watch

- **Live web values** — panels update every ~5s without a page reload
  (topcoat shard re-rendering server-side; values change because the mock
  upstream drifts).
- **Threshold colors** — `disk-root` goes green → yellow → red as `/` fills;
  `bank-balance-thresholds` uses the opposite direction (more USD is greener).
- **Grid layouts** — two layouts with multiple rows, a spacer, a colspan-2 gap
  and a custom pane title ("Balance bands"); see `config.toml`.
- **Generalized panes** — "Server" combines three sections in one pane:
  `disk-root` as `main` (large text, colored by its own status, its own
  history-bar preview, always-shown "updated ago"), `domain-expiry` as
  `secondary` (a plain colored value+unit, linked to its own `/logs/<source>`
  view — multiple `secondary` members render side by side in one row), and
  `disk-home` as a `table` row labeled "home" (independently colored value,
  "updated ago" only once it falls stale). The pane's own border tracks the
  worst member across all three sections (red > yellow > green) — its
  background stays neutral. (A pane with only `main` set and nothing else
  renders exactly like a plain single-source panel instead, with its own
  colored border and background — see `bank-balance-thresholds` below.)
- **Source titles** — `disk-root` declares `title = "Root filesystem"`; its
  bare-id `main` entry in the "Server" pane shows that title instead of the
  raw id `disk-root`.
- **History bar opt-out** — `disk-home` (in the pane's `table` section) is
  threshold-banded but declares `show_history = false`, so its table row
  shows no bar; compare it against `disk-root` in `main`, which is banded the
  same way and does show one. `domain-expiry` in `secondary` shows no bar
  either, but for a different reason — `secondary` never renders one,
  regardless of thresholds, since it only ever shows a plain colored value.
- **Value formats** — `weekly-report` renders markdown (headings, bold, lists)
  and `status-json` pretty-prints its JSON payload.
- **Failing source** — `dead-service` points at a closed port; after 2
  consecutive failures its panel turns red and `health` reports `failing`,
  with a plain "failing" label alongside the color even though it declares no
  threshold bands. Other sources keep collecting on schedule.
- **Setup commands** — `tunneled-service` has a setup command that fails until
  you run `touch /tmp/bd-tunnel-up`; watch it retry on its schedule, then turn
  healthy and start fetching without a daemon restart.
- **Config-relative commands** — `load-average` runs `scripts/load-average.sh`
  via a relative path; it resolves against `demo/` (this config file's
  directory) no matter where the daemon was launched from.
- **Stale source** — stop the mock server (`kill %1`); after 60s without a
  success, `bank-balance` and `work-hours` turn amber (`stale`).
- **Restart persistence** — Ctrl-C the daemon, restart it: history is still
  there.

## Storage

Everything lands in one DuckDB file, `demo/dashboard.duckdb`, Open it with any
DuckDB client while the daemon is stopped — or use the daemon's API:

```sh
duckdb demo/dashboard.duckdb \
  "SELECT source, value, ts FROM readings ORDER BY ts_epoch DESC LIMIT 5"
curl -s localhost:18420/api/sources/bank-balance/history | head
```

Tables: `readings`, `fetch_logs`, `health_events`.

## How it fits together

```
config.toml ──► sources (http / bash) ──► scheduler (tokio, per-source tasks)
                     │                          │
                     │                          ▼
                     │                 fetch_logs / readings / health_events
                     │                          │
                     ▼                          ▼
              topcoat web UI ◄────────── DuckDB (single file)
              JSON API (/api/…)                ▲
                     │                         │ direct read-only
             CLI / TUI ────────────────────────┘   (--daemon routes over the API)
```

- Sources are declared in TOML — generic `http` (URL + dotted JSON selector) and
  `bash` (command stdout) types; no code changes to add a data point.
- Layouts are config too: `[[layouts]]` lists source names per panel; TUI and
  web render the same definition.
- One binary, four surfaces. Planning docs live in `openspec/`.
