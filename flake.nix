{
  description = "barduck: single-binary home dashboard daemon";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # The Tailwind CLI release `topcoat-tailwind` downloads by default at
      # build time (see topcoat_tailwind::build::DEFAULT_VERSION, currently
      # pinned to this version by the `topcoat = "0.6.2"` dependency).
      # Pre-fetched here as a fixed-output derivation so the actual `cargo
      # build` stays network-free inside Nix's build sandbox.
      tailwindVersion = "4.3.2";
      tailwindPlatforms = {
        x86_64-linux = {
          asset = "tailwindcss-linux-x64";
          hash = "sha256-UDbE+0Mo4LzbtgZccNislFLg1MlHETp4io+U/TkEJcE=";
        };
        aarch64-linux = {
          asset = "tailwindcss-linux-arm64";
          hash = "sha256-OU3czCQCz6Or2X37pW81h3gaPW5s5m5lzq2hS+t2ZLg=";
        };
      };
    in
    {
      packages = forAllSystems (
        pkgs:
        let
          system = pkgs.system;
          tw = tailwindPlatforms.${system};
          tailwindBin = pkgs.fetchurl {
            url = "https://github.com/tailwindlabs/tailwindcss/releases/download/v${tailwindVersion}/${tw.asset}";
            hash = tw.hash;
          };

          # `topcoat asset bundle` (invoked below as a build step of the main
          # package) is a separate dev-tool binary from the `topcoat-cli`
          # crate, not a dependency of barduck itself.
          topcoat-cli = pkgs.rustPlatform.buildRustPackage {
            pname = "topcoat-cli";
            version = "0.6.2";
            src = pkgs.fetchCrate {
              pname = "topcoat-cli";
              version = "0.6.2";
              hash = "sha256-+6DKc2yjKwju64iokWV1EUXf62GGj1W32kp7E7WVKdM=";
            };
            cargoHash = "sha256-CDnvF0CMkpjIK0dDhuxIfpm0PnyEjFvbIjWhgSfFk1w=";
            doCheck = false;
          };
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "barduck";
            version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
            src = self;

            cargoLock.lockFile = ./Cargo.lock;

            # Links nixpkgs' libduckdb (same 1.5.5 the pinned duckdb-rs
            # 1.10505.0 targets — duckdb-rs encodes it as 1.MAJOR_MINOR_PATCH,
            # see libduckdb-sys's build.rs) instead of compiling DuckDB's
            # bundled C++ source. autoPatchelfHook fixes up the resulting
            # binary's rpath to find it at runtime.
            buildInputs = [ pkgs.duckdb ];
            nativeBuildInputs = [
              pkgs.autoPatchelfHook
              topcoat-cli
            ];

            # `topcoat-tailwind`'s build.rs (pulled in transitively via the
            # `topcoat` dependency's build script) downloads the Tailwind CLI
            # into a shared cache under $CARGO_TARGET_DIR on first use.
            # Pre-seed that cache with the fixed-output derivation above so
            # the real build never touches the network.
            preBuild = ''
              export CARGO_TARGET_DIR="$PWD/target"
              cache_dir="$CARGO_TARGET_DIR/topcoat/cache/tailwind"
              mkdir -p "$cache_dir"
              install -m755 ${tailwindBin} "$cache_dir/tailwindcss-${tailwindVersion}-${tw.asset}"
            '';

            # Produces the `assets/` directory the web UI reads at runtime
            # (see src/lib.rs: AssetBundle::load() looks next to the running
            # executable) — without it, daemon mode serves the API/TUI fine
            # but web UI pages render with no styling.
            postInstall = ''
              topcoat asset bundle --release --out "$out/bin/assets"
            '';

            doCheck = false;

            meta.mainProgram = "barduck";
          };
        }
      );

      homeManagerModules.default =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.barduck;
          format = pkgs.formats.toml { };
          # The app chdirs to the config file's directory at startup and
          # resolves `database_path` relative to it, so a default of just
          # "dashboard.duckdb" would try to write into the (read-only) Nix
          # store wherever `settings` doesn't override it explicitly.
          stateDir = "${config.xdg.stateHome}/barduck";
          settings = lib.recursiveUpdate { database_path = "${stateDir}/dashboard.duckdb"; } (
            cfg.settings // { listen = "${cfg.host}:${toString cfg.port}"; }
          );
          configFile = format.generate "barduck-config.toml" settings;
        in
        {
          options.services.barduck = {
            enable = lib.mkEnableOption "the barduck systemd user service";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.system}.default;
              description = "The barduck package to run.";
            };

            port = lib.mkOption {
              type = lib.types.port;
              default = 8420;
              description = "TCP port the daemon's web UI and HTTP API listen on.";
            };

            host = lib.mkOption {
              type = lib.types.str;
              default = "127.0.0.1";
              description = "Address the daemon binds to.";
            };

            settings = lib.mkOption {
              type = format.type;
              default = { };
              description = ''
                barduck config.toml contents (sources, layouts, etc. —
                see demo/config.toml for the full schema). `listen` is set
                from `port`/`host` and does not need to be repeated here.
              '';
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.user.services.barduck = {
              Unit.Description = "barduck daemon";
              Install.WantedBy = [ "default.target" ];
              Service = {
                ExecStart = "${lib.getExe cfg.package} --config ${configFile} daemon";
                Restart = "on-failure";
                StateDirectory = "barduck";
              };
            };
          };
        };
    };
}
