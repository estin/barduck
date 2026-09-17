# barduck — lightweight self-hosted personal dashboard

> A lightweight, self-hosted personal dashboard that collects values from
> shell commands and scripts (or receives them via HTTP push), stores
> history in embedded DuckDB, and exposes it through a live web UI, TUI,
> CLI, and JSON API — all as a single binary.

Barduck is a simple alternative to heavyweight monitoring/dashboard stacks
(Prometheus + Grafana and similar) for personal and home-server data: disk
usage, service health, a bank balance, domain expiry, load averages,
webhook events, or anything else you can express as a shell command or push
over HTTP.

> Personal project, developed with LLM coding agents using
> [OpenSpec](openspec/) for spec-driven changes. Experimental and provided
> as-is.
>
> Name: "board" (dashboard) + "duck" (DuckDB), also a nod to Russian
> "бардак" (bardak, "mess") — it tames scattered home stats into one board.

Config-defined sources (oneshot `query` commands on a schedule, continuous
`stream` commands emitting JSON lines, or push-based `ingest` sources fed
over HTTP) are stored in DuckDB and shown via a live web UI, TUI, CLI, and
JSON API. A `query` source can also declare `children`, turning one
command's JSON-array output into several independently-displayed,
independently-healthed values (a composite source — see
[SKILL.md](SKILL.md#composite-sources)). See [demo/README.md](demo/README.md)
for a runnable tour.

- Config: TOML (`sources`, `layouts`) — adding a data point needs no code
- Storage: embedded DuckDB, one file, plain SQL accessible
- Surfaces: `daemon` (collector + API + web), `tui` (ratatui), query commands,
  all with direct-DB or daemon-backed modes
- Off-schedule refresh: `barduck poll -s <name>` (or a panel's "poll now"
  control) fetches and stores immediately; `barduck fetch -s <name>` is the
  debug counterpart that writes nothing
- Planning artifacts and specs: [`openspec/`](openspec/)

## When should I use barduck?

barduck is a good fit if you:

- want a personal or home-server dashboard;
- have data available through shell commands, scripts, or HTTP push;
- want to keep historical data locally, queryable with plain SQL;
- prefer an embedded database and a single binary over running a stack;
- want both a web UI and a TUI;
- don't need Prometheus compatibility or a full observability platform.

barduck is probably not a good fit if you need:

- large-scale production observability;
- distributed metrics collection;
- enterprise RBAC or multi-tenant dashboards.

## Example sources

- **Home server**: disk usage, load average, service health, Docker status
- **Personal**: bank balance, domain expiry, weekly work hours
- **Events**: webhook/event data pushed to `/api/ingest`

See [demo/config.toml](demo/config.toml) for these as runnable examples.

## barduck vs Grafana

Grafana is an excellent choice for observability and production monitoring,
but it can be more than a small personal or home-server dashboard needs.
barduck is a simpler alternative when data comes from a few shell commands
or scripts, historical values need to be stored locally, a single binary is
preferred, and Prometheus is unnecessary.

| | barduck | Grafana |
|---|---|---|
| Deployment | Single binary | Typically Grafana + a separate datasource/database |
| Storage | Embedded DuckDB (one file, plain SQL) | External database/time-series backend |
| Configuration | TOML | UI + datasource configuration |
| Data collection | Built-in (shell commands, HTTP push) | Usually via a separate datasource/exporter |
| TUI | Yes | No |
| Target | Personal / home-server | Observability / production |

## Platforms and commands

Barduck uses `sh -c` on Linux/macOS and native `cmd.exe /D /S /C` on
Windows. Query, setup, and stream commands run in the config file's
directory. Commands must use the host shell's syntax and installed tools;
the POSIX demo scripts are not native Windows examples. PowerShell scripts
can be invoked explicitly, for example `powershell.exe -NoProfile -File scripts/metric.ps1`.

Cancellation, output errors, and stream shutdown terminate the owned
process tree: a process group on Unix, a Job Object on Windows. Commands
must not deliberately detach from the Unix process group. No Git Bash or
WSL is required for Windows command execution.

Build with stable Rust and DuckDB available to the linker, or use
`cargo build --release --features bundled` with a native C++ toolchain.
The build downloads the host Tailwind executable. Web deployments also
need the Topcoat CLI (`cargo install topcoat-cli --version 0.6.2`) and
`topcoat asset bundle --release`; keep `target/release/assets` beside the
binary. The Nix/home-manager service below is Linux/systemd-specific.

CI runs native command execution and parent/descendant cleanup regressions
on Windows and macOS, alongside the existing Ubuntu suite.

## Nix / home-manager

`flake.nix` exposes `packages.<system>.default` (the binary, built against
nixpkgs' `duckdb` with the web UI's Tailwind assets pre-bundled) and
`homeManagerModules.default`, a systemd user service:

```nix
{
  inputs.barduck.url = "github:<you>/barduck"; # or "path:./barduck"

  # in your home-manager config:
  imports = [ inputs.barduck.homeManagerModules.default ];
  services.barduck = {
    enable = true;
    port = 8420; # -> listen = "127.0.0.1:8420"
    settings.sources = [ /* see demo/config.toml for the schema */ ];
  };
}
```

## Documentation

- [SKILL.md](SKILL.md) — source types, config reference
- [docs/comparison.md](docs/comparison.md) — barduck vs Grafana, in more detail
- [demo/README.md](demo/README.md) — runnable tour

## Alternative projects

- [wtfutil](https://wtfutil.com/)
- [sampler](https://sampler.dev/)
- [glances](https://nicolargo.github.io/glances/)
- [homepage](https://gethomepage.dev/)
