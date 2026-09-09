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
live web UI, TUI, CLI, and JSON API. See [demo/README.md](demo/README.md) for
a runnable tour.

- Config: TOML (`sources`, `layouts`) — adding a data point needs no code
- Storage: embedded DuckDB, one file, plain SQL accessible
- Surfaces: `daemon` (collector + API + web), `tui` (ratatui), query commands,
  all with direct-DB or daemon-backed modes
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
