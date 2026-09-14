# barduck

> Personal project, developed with LLM coding agents using
> [OpenSpec](openspec/) for spec-driven changes. Not intended for outside
> use or support.
>
> Name: "board" (dashboard) + "duck" (DuckDB), also a nod to Russian
> "бардак" (bardak, "mess") — it tames scattered home stats into one board.

Single-binary home dashboard: collects values from config-defined shell
sources (oneshot `query` commands on a schedule, continuous `stream`
commands emitting JSON lines), stores them in DuckDB, and shows them via a
live web UI, TUI, CLI, and JSON API. A `query` source can also declare
`children`, turning one command's JSON-array output into several
independently-displayed, independently-healthed values (a composite
source — see [SKILL.md](SKILL.md#composite-sources)). See
[demo/README.md](demo/README.md) for a runnable tour.

- Config: TOML (`sources`, `layouts`) — adding a data point needs no code
- Storage: embedded DuckDB, one file, plain SQL accessible
- Surfaces: `daemon` (collector + API + web), `tui` (ratatui), query commands,
  all with direct-DB or daemon-backed modes
- Off-schedule refresh: `barduck poll -s <name>` (or a panel's "poll now"
  control) fetches and stores immediately; `barduck fetch -s <name>` is the
  debug counterpart that writes nothing
- Planning artifacts and specs: [`openspec/`](openspec/)

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

## Alternative projects

- [wtfutil](https://wtfutil.com/)
- [sampler](https://sampler.dev/)
- [glances](https://nicolargo.github.io/glances/)
- [homepage](https://gethomepage.dev/)
