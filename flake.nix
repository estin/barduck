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

              # `topcoat asset bundle` (postInstall, below) reruns `cargo
              # build` itself to scan for assets, deliberately stripping
              # every CARGO_*/RUSTC*/RUSTFLAGS env var first (so a `cargo
              # run`-invoked topcoat-cli doesn't leak its own wrapper's env
              # into the rebuild). That drops cargoBuildHook's explicit
              # `--target ${pkgs.stdenv.hostPlatform.rust.rustcTarget}`, so
              # the rebuild lands in `target/release/` instead of
              # `target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/`
              # and skips the `[target."${pkgs.stdenv.hostPlatform.rust.rustcTarget}"]`
              # rustflags cargoSetupPostUnpackHook wrote to .cargo/config.toml
              # — a different fingerprint, so cargo fully recompiles, and
              # `stylesheet!()`'s `asset!(concat!(env!("OUT_DIR"), ...))`
              # (topcoat-tailwind/src/stylesheet.rs) bakes in a different
              # OUT_DIR path each time, producing a different AssetId than
              # the one actually installed at $out/bin/barduck: the daemon
              # then fails at runtime with "failed to resolve asset ... in
              # the asset catalog". `target` in .cargo/config.toml (unlike an
              # env var) survives the stripping and isn't overridden by an
              # explicit --target flag, so it pins both builds to the same
              # layout and fingerprint — the second "build" becomes a cache
              # hit that just rescans the exact binary already installed.
              mkdir -p .cargo
              cat >> .cargo/config.toml <<EOF
              [build]
              target = "${pkgs.stdenv.hostPlatform.rust.rustcTarget}"
              EOF
            '';

            # Produces the `assets/` directory the web UI reads at runtime
            # (see src/lib.rs: AssetBundle::load() looks next to the running
            # executable) — without it, daemon mode serves the API/TUI fine
            # but web UI pages render with no styling.
            postInstall = ''
              topcoat asset bundle --release --out "$out/bin/assets"

              # `topcoat asset bundle` scans a binary it rebuilds itself
              # (see the .cargo/config.toml note in preBuild above) rather
              # than the one cargoInstallHook already copied to
              # $out/bin/barduck moments earlier. Even with that fix keeping
              # both builds on the same target layout and fingerprint, nothing
              # guarantees the two invocations link a byte-identical
              # executable (a build script can still rerun and relink,
              # e.g. topcoat-tailwind's build.rs re-invoking the Tailwind
              # CLI on every `cargo build`). Re-install the exact executable
              # the bundler just scanned over $out/bin/barduck so the
              # shipped binary and its asset catalog can never disagree,
              # regardless of such drift.
              install -m755 "$CARGO_TARGET_DIR/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/barduck" "$out/bin/barduck"
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
