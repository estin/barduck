# barduck vs other self-hosted dashboards

This document compares barduck against other self-hosted dashboard /
monitoring tools that people reach for when a full observability stack
(Grafana + Prometheus, etc.) is too much. Each tool fits a different
shape of problem — these are buying guides, not rankings.

A consolidated feature matrix is below, followed by per-tool guidance.

## Feature comparison

| | barduck | Grafana | wtfutil | sampler | glances | homepage | netdata |
|---|---|---|---|---|---|---|---|
| Deployment | Single binary | Grafana + separate DB/datasource | Single binary | Single binary | Python (pip) / package | Single Docker container | Single binary (kickstart) / Docker |
| Storage | Embedded DuckDB (one file, plain SQL) | External DB / time-series backend | None | None | None (exports only) | None | Local round-robin DB (per-second metrics) |
| Configuration | TOML | UI + datasource config | YAML | YAML | INI / flags | YAML (+ docker labels) | INI (`netdata.conf`, alarm `.conf`, `python.d`) |
| Data collection | Scheduled shell `query`; streaming `stream`; HTTP push `/api/ingest` | Via datasource/exporter (Prometheus, InfluxDB, …) | Built-in API/service modules (GitHub, Jira, calendars, …) | Shell commands run on a configured rate | System metrics via psutil, auto-discovered | Service status checks (100+ integrations) + custom API widgets | Auto-discovered collectors (system/apps/containers, 1000+); custom shell collectors via plugins |
| Historical data | Yes — local (DuckDB) | Yes — via datasource | No | No (current session only) | No (exports to external sinks only) | No | Yes — local (round-robin) |
| Composite metrics | Yes — one source can emit several tracked values (`children`) | Per-datasource dependent | Per-module only | Multiple series per chart | No | No | Yes — custom charts from collectors |
| TUI | Yes (`barduck tui`, ratatui) | No | Yes | Yes | Yes (curses) | No | No (CLI control only) |
| CLI | Yes (`latest`, `logs`, `fetch`, `poll`, JSON) | Limited | Launcher/flags only | Launcher only | Yes (JSON/CSV output) | No | Yes (`netdatacli`, JSON API) |
| Web UI | Yes (live-updating) | Yes | No | No | Yes (built-in server) | Yes | Yes (local + Netdata Cloud) |
| Target | Personal/home-server dashboards with history | Observability / production at scale | Terminal dashboard of live service info | Quick visualization of shell output (dev/experimentation) | System/resource monitoring | App launcher / service status board | Real-time infra/app monitoring + alerting |

barduck-only capabilities of note that few of these match: a TUI **and**
a web UI from one binary, an embedded SQL store for history, an HTTP
push ingest endpoint, CLI that is scriptable/JSON-first, and per-source
composite values.

# barduck vs Grafana

Grafana is an excellent choice for observability and production monitoring,
but it can be unnecessarily complex for a small personal or home-server
dashboard.

barduck is a simpler alternative when:

- data comes from a few shell commands, scripts, or HTTP push;
- historical values need to be stored locally;
- a single binary is preferred over a stack of components;
- Prometheus is unnecessary;
- dashboards are primarily for personal use;
- embedded DuckDB is sufficient as the storage layer, and plain SQL access
  to that data is useful.

## Choose barduck when

- you want a single binary with no separate database or exporter to run;
- your data sources are shell commands, scripts, or HTTP push;
- you want an embedded, SQL-queryable store instead of a time-series backend;
- you want both a web UI and a terminal UI;
- you want a scriptable CLI with JSON output for `latest`/`logs`/`fetch`/`poll`;
- you don't need Prometheus compatibility.

## Choose Grafana when

- you need production-grade observability at scale;
- you need Prometheus (or another established datasource ecosystem)
  compatibility;
- you need multi-tenant dashboards, alerting at scale, or enterprise RBAC;
- your team already has metrics infrastructure Grafana can plug into.

# barduck vs wtfutil

[WTF](https://wtfutil.com/) is a terminal dashboard of live "modules" that
talk to services (GitHub, Jira, calendars, etc.) and render the result as
read-only widgets. It is a great "ops status board in your terminal" — but
it is not a metrics store.

## Choose barduck when

- you want your data persisted with history, not just the latest poll;
- your sources are arbitrary shell commands or an HTTP push endpoint, rather
  than a curated list of APIs;
- you want a web UI (not only a terminal) and a scriptable JSON CLI;
- you want to query collected values later with SQL.

## Choose wtfutil when

- you mainly want one screen of live service status (CI state, calendars,
  tickets, balances) in your terminal;
- your data is available through one of its built-in API modules;
- you do not need to store or query history.

# barduck vs sampler

[Sampler](https://sampler.dev/) visualizes the output of shell commands as
charts/gauges/sparklines, configured in YAML. It is ideal for quickly
watching a command's output, and — like barduck — treats shell commands as
first-class sources. The differences are about storage and reach.

## Choose barduck when

- you want command output persisted over time, not just shown for the
  lifetime of the process;
- you want a web UI and an HTTP push path in addition to a TUI;
- you want to correlate several values from one command into independently
  tracked series (`children`) and query them with SQL afterwards.

## Choose sampler when

- you want fast, throwaway visualization of a command's output with no
  server, no database, and no extra moving parts;
- a terminal-only chart that you can resize and reposition live is exactly
  what you need.

# barduck vs glances

[Glances](https://nicolargo.github.io/glances/) is a curses (and web)
system monitor — CPU, memory, processes, disks, network, Docker, etc.
It reads the OS itself via psutil; it does not run arbitrary commands as
sources.

## Choose barduck when

- your dashboard is about values your own scripts or HTTP endpoints
  produce, not the host's resources;
- you want those values stored with history and queried with SQL;
- you want both TUI and web access to the same stored data.

## Choose glances when

- the thing you want to watch is the host: processes, CPU, memory, disks,
  network, containers;
- a single `pip install glances` live view (terminal or browser) is enough,
  and you either don't need history or are happy exporting to an external
  sink (InfluxDB, CSV, …).

# barduck vs homepage

[Homepage](https://gethomepage.dev/) is a self-hosted application dashboard
/ status board: links to your apps plus status widgets for 100+ services,
configured via YAML or Docker labels. It is a launcher and status board,
not a metrics/time-series tool.

## Choose barduck when

- you want to collect and *store* numeric values over time (shell output,
  HTTP pushes), not just show the current state of apps/services;
- you want those values queryable with SQL and rendered as historical
  charts in a web UI.

## Choose homepage when

- you mainly want a homepage of links to your apps and a quick status read
  on services;
- you prefer a single Docker container configured via docker labels / a
  YAML file and are happy with live, non-historical widgets.

# barduck vs netdata

[Netdata](https://www.netdata.cloud/) is a real-time monitoring agent with a
local first-second-granularity database, a web dashboard, alerting, and
auto-discovered collectors for systems, containers, and 1000+ services. It
shares barduck's "collect values, keep history, show a dashboard" shape —
but is purpose-built for infrastructure/application metrics, not arbitrary
shell commands or HTTP pushes.

## Choose barduck when

- your sources are arbitrary shell commands (one of which can report
  several related values via `children`) or HTTP pushes to `/api/ingest`;
- you want the data in embedded DuckDB and query it with plain SQL;
- you want a TUI (`barduck tui`) alongside the web UI and a scriptable CLI;
- you are fine with a light personal dashboard and don't need auto-discovery
  of system/container metrics at scale or ML-based alerting.

## Choose netdata when

- you want rich, per-second system and application monitoring out of the
  box with auto-discovered collectors (Docker, Kubernetes, databases, web
  servers, …);
- you need built-in alerting/alarm engines and, optionally, team
  collaboration via Netdata Cloud;
- you want real-time (1-second) dashboards with anomaly detection, and are
  OK with it being web-first and more opinionated about what it monitors.

## Other tools worth a look

A few more self-hosted dashboard alternatives people mix into this space:

- [Dashy](https://github.com/Lissy93/dashy) — status/dashboard board, single
  Docker container, themed status cards and widgets (status, not metrics history).
- [Flame](https://github.com/pawelmalak/flame) — self-hosted startpage for
  your server. Easily manage apps and bookmarks with built-in editors.
- [Glance](https://github.com/glanceapp/glance) — lightweight (<20 MB) single
  binary dashboard of feeds/widgets (RSS, Reddit, Hacker News, YouTube,
  markets, Docker status, custom API); web UI only, no TUI or history store.
- [Uptime Kuma](https://github.com/louislam/uptime-kuma) — self-hosted
  uptime/status monitor for HTTP/TCP/port pings with history.

## Sources

- barduck: [llms.txt](/llms.txt) · [SKILL.md](https://github.com/estin/barduck/blob/master/SKILL.md) · [demo](https://github.com/estin/barduck/tree/master/demo)
- [Grafana](https://grafana.com/) — product docs
- [WTF](https://wtfutil.com/) — terminal dashboard, modules & configuration
- [Sampler](https://sampler.dev/) — shell command visualization (GitHub README)
- [Glances](https://nicolargo.github.io/glances/) — system monitor (readthedocs)
- [Homepage](https://gethomepage.dev/) — application dashboard
- [Netdata](https://learn.netdata.cloud/) — real-time monitoring agent (Learn Netdata)
