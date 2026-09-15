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

## Feature comparison

| | barduck | Grafana |
|---|---|---|
| Deployment | Single binary | Grafana + typically a separate datasource/database |
| Storage | Embedded DuckDB (one file, plain SQL) | External database/time-series backend |
| Configuration | TOML | UI + datasource configuration |
| Data collection | Built-in: scheduled shell commands (`query`), streaming commands (`stream`), HTTP push (`ingest`) | Usually via a separate datasource/exporter |
| Composite metrics | One command can report several related values (`children`) | Per-datasource, depends on the exporter |
| TUI | Yes (`barduck tui`) | No |
| CLI | Yes (`latest`, `logs`, `fetch`, `poll`, JSON output) | Limited |
| Off-schedule refresh | `barduck poll -s <name>` / panel "poll now" | Depends on datasource |
| Target | Personal / home-server dashboards | Observability / production monitoring |

## Choose barduck when

- you want a single binary with no separate database or exporter to run;
- your data sources are shell commands, scripts, or HTTP push;
- you want an embedded, SQL-queryable store instead of a time-series backend;
- you want both a web UI and a terminal UI;
- you don't need Prometheus compatibility.

## Choose Grafana when

- you need production-grade observability at scale;
- you need Prometheus (or another established datasource ecosystem)
  compatibility;
- you need multi-tenant dashboards, alerting at scale, or enterprise RBAC;
- your team already has metrics infrastructure Grafana can plug into.

barduck and Grafana solve different problems — this isn't a
"barduck is better than Grafana" claim, just a guide to which one fits a
given use case.
