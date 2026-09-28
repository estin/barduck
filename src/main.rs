use anyhow::{Context as _, Result};
use barduck::{cli_report, config, db::Db, query::Backend, run_daemon, tui};
use clap::{Args, Parser, Subcommand};
use std::io::{IsTerminal as _, Write as _};

#[derive(Parser)]
#[command(name = "barduck", version, about = "Single-binary home dashboard")]
struct Cli {
    /// Path to the TOML config file. When omitted, resolution order is:
    /// `BARDUCK_CONFIG`, then `barduck/config.toml` under `XDG_CONFIG_HOME`
    /// (or `~/.config`), then `./config.toml` in the working directory.
    #[arg(long, short, global = true)]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Default)]
struct QueryArgs {
    /// Output JSON instead of a table.
    #[arg(long)]
    json: bool,
    /// Route queries through the running daemon's HTTP API.
    #[arg(long)]
    daemon: bool,
    /// Only show rows for these source names (repeatable).
    #[arg(long, short = 's')]
    source: Vec<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the collector + HTTP API + web UI in one process.
    Daemon,
    /// Terminal dashboard (ratatui).
    Tui {
        /// Route queries through the daemon instead of reading the DB file.
        #[arg(long)]
        daemon: bool,
    },
    /// Latest value per source.
    Latest {
        #[command(flatten)]
        flags: QueryArgs,
        /// Exclude markdown-format ("text") sources entirely, instead of
        /// showing them in a separate section after the table.
        #[arg(long)]
        no_text: bool,
    },
    /// Recent fetch logs.
    Logs {
        #[arg(long, default_value_t = 50)]
        limit: i64,
        #[command(flatten)]
        flags: QueryArgs,
    },
    /// Permanently delete all collected data (readings, fetch logs, health
    /// events) and start from an empty database. Prompts for confirmation.
    Reset {
        /// Skip the confirmation prompt.
        #[arg(long, short = 'y')]
        yes: bool,
        /// Output JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Fetch sources now, ignoring their schedules, and store the results
    /// exactly as a scheduled fetch does. The writing counterpart to
    /// `fetch`, which only prints.
    Poll {
        /// Source to poll (repeatable; at least one required).
        #[arg(long, short = 's', required = true)]
        source: Vec<String>,
        /// Output JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
        /// Route the poll through the running daemon's HTTP API. Without
        /// it, a daemon already holding the database is detected and used
        /// anyway; otherwise the fetch runs in this process.
        #[arg(long)]
        daemon: bool,
    },
    /// Run one source once and print the parsed result without touching
    /// the database (debug; use `poll` to fetch *and* store).
    Fetch {
        /// Which source to run.
        #[arg(long, short = 's')]
        source: String,
        /// Output JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
    /// Print the skill document for LLM agents.
    Skill,
}

/// Asks the user to type "yes" on stdin; any other input (including empty)
/// answers no. Refuses to prompt at all when stdin is not a terminal — the
/// reset it guards is an irreversible `DROP TABLE`, and a piped or heredoc
/// stdin is exactly how `echo yes | barduck reset` reaches the wipe by
/// accident. `--yes` is the explicit non-interactive path.
fn confirm(prompt: &str) -> Result<bool> {
    if !std::io::stdin().is_terminal() {
        println!("Refusing to prompt on a non-interactive stdin; re-run with --yes to confirm.");
        return Ok(false);
    }
    print!("{prompt} [yes/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer)? == 0 {
        return Ok(false);
    }
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}

/// Resolves which config file to load (spec: source-configuration — Config
/// file defines sources): explicit `--config` first, else `BARDUCK_CONFIG`
/// when non-empty, else the XDG default when that file exists, else
/// `./config.toml` when it exists, else an error naming the paths tried.
fn resolve_config_path(
    flag: Option<&std::path::Path>,
    env: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<std::path::PathBuf> {
    if let Some(p) = flag {
        return Ok(p.to_path_buf());
    }
    if let Some(v) = env("BARDUCK_CONFIG")
        && !v.trim().is_empty()
    {
        return Ok(std::path::PathBuf::from(v));
    }
    let mut tried = Vec::new();
    if let Some(xdg) = xdg_default_config_path(env) {
        if xdg.exists() {
            return Ok(xdg);
        }
        tried.push(xdg);
    }
    let cwd = std::path::PathBuf::from("config.toml");
    if cwd.exists() {
        return Ok(cwd);
    }
    tried.push(cwd);
    let tried = tried
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!("no config file found (tried: {tried})");
}

/// `barduck/config.toml` under `XDG_CONFIG_HOME` (non-empty) or
/// `$HOME/.config`; `None` when `HOME` is missing so the caller falls
/// through to the working-directory candidate instead of panicking.
fn xdg_default_config_path(
    env: &mut dyn FnMut(&str) -> Option<String>,
) -> Option<std::path::PathBuf> {
    let base = match env("XDG_CONFIG_HOME") {
        Some(v) if !v.trim().is_empty() => std::path::PathBuf::from(v),
        _ => std::path::PathBuf::from(env("HOME")?).join(".config"),
    };
    Some(base.join("barduck").join("config.toml"))
}

/// Live process-environment lookup for [`resolve_config_path`]; tests pass
/// an injectable stand-in instead (`std::env::set_var` is `unsafe` as of
/// the 2024 edition, and this project denies `unsafe_code`).
struct EnvLookup;

impl EnvLookup {
    fn live() -> impl FnMut(&str) -> Option<String> {
        |name| std::env::var(name).ok()
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();

    // Handle skill command before loading config — no config file needed.
    if let Cmd::Skill = cli.cmd {
        println!("{}", include_str!("../SKILL.md"));
        return Ok(());
    }

    let unresolved = resolve_config_path(cli.config.as_deref(), &mut EnvLookup::live())?;
    eprintln!("using config: {}", unresolved.display());
    let config_path = std::fs::canonicalize(&unresolved)
        .with_context(|| format!("resolving config path {}", unresolved.display()))?;
    // `config::load` resolves `database_path` and records `config_dir` itself
    // — scripts spawn with that directory as their own working directory
    // (spec: source-configuration — config-relative working directory), so
    // the process's own current directory never needs to change (leaving
    // `std::env::current_dir()` correct for any other code in-process).
    let cfg = config::load(&config_path)?;

    match cli.cmd {
        Cmd::Daemon => multi_rt()?.block_on(run_daemon(cfg)),
        Cmd::Tui { daemon } => tui::run(&Backend::new(&cfg, daemon)?, &cfg),
        Cmd::Latest { flags, no_text } => rt()?.block_on(cli_report::print_latest(
            &Backend::new(&cfg, flags.daemon)?,
            &cfg,
            &flags.source,
            flags.json,
            no_text,
        )),
        Cmd::Logs { limit, flags } => rt()?.block_on(cli_report::print_logs(
            &Backend::new(&cfg, flags.daemon)?,
            &cfg,
            limit,
            &flags.source,
            flags.json,
            &mut std::io::stdout(),
        )),
        Cmd::Reset { yes, json } => {
            // A running daemon holds the database's `DuckDB` lock for its
            // whole lifetime, so the open below would spend seconds retrying
            // and then fail with a raw lock error. Say what to do instead.
            if Db::open_ro(&cfg.database_path)?.is_locked_by_another_process() {
                anyhow::bail!(
                    "{} is locked by another process — stop the running daemon before resetting",
                    cfg.database_path.display()
                );
            }
            if !yes
                && !confirm(&format!(
                    "This will permanently delete all data in {}.",
                    cfg.database_path.display()
                ))?
            {
                println!("Aborted.");
                return Ok(());
            }
            Db::open_rw(&cfg.database_path)?.reset()?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({"status": "reset", "database": cfg.database_path.display().to_string()})
                );
            } else {
                println!("Database reset: {}", cfg.database_path.display());
            }
            Ok(())
        }
        Cmd::Poll {
            source,
            json,
            daemon,
        } => rt()?.block_on(cli_report::print_poll(
            &Backend::new(&cfg, daemon)?,
            &cfg,
            &source,
            json,
            &mut std::io::stdout(),
        )),
        Cmd::Fetch { source, json } => rt()?.block_on(cli_report::print_fetch(&cfg, &source, json)),
        Cmd::Skill => unreachable!(),
    }
}

fn rt() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building runtime")
}

fn multi_rt() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building runtime")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic)]
    use super::*;

    /// (spec: cli — Source debug fetch command)
    #[test]
    fn fetch_takes_source_flag_not_positional() {
        assert!(Cli::try_parse_from(["barduck", "fetch", "--source", "cpu"]).is_ok());
        assert!(Cli::try_parse_from(["barduck", "fetch", "-s", "cpu"]).is_ok());
        // Missing flag and positional form both fail.
        assert!(Cli::try_parse_from(["barduck", "fetch"]).is_err());
        assert!(Cli::try_parse_from(["barduck", "fetch", "cpu"]).is_err());
    }

    /// (spec: cli — Query commands)
    #[test]
    fn removed_history_and_health_rejected() {
        assert!(Cli::try_parse_from(["barduck", "history", "cpu"]).is_err());
        assert!(Cli::try_parse_from(["barduck", "health"]).is_err());
    }

    /// (spec: cli — Reset reports machine-readable result)
    /// (spec: cli — Force poll command)
    #[test]
    fn poll_requires_at_least_one_repeatable_source() {
        let cli = Cli::try_parse_from(["barduck", "poll", "-s", "a", "-s", "b"]).unwrap();
        match cli.cmd {
            Cmd::Poll { source, .. } => assert_eq!(source, ["a", "b"]),
            _ => panic!("expected poll"),
        }
        assert!(Cli::try_parse_from(["barduck", "poll", "--source", "a"]).is_ok());
        assert!(Cli::try_parse_from(["barduck", "poll", "--json", "--daemon", "-s", "a"]).is_ok());
        // No source, and a positional in place of the flag, both rejected.
        assert!(Cli::try_parse_from(["barduck", "poll"]).is_err());
        assert!(Cli::try_parse_from(["barduck", "poll", "a"]).is_err());
    }

    #[test]
    fn reset_accepts_json_flag() {
        let cli = Cli::try_parse_from(["barduck", "reset", "--json", "--yes"]).unwrap();
        assert!(matches!(cli.cmd, Cmd::Reset { json: true, .. }));
    }

    /// (spec: source-configuration — Config file defines sources)
    #[test]
    fn config_flag_is_optional_and_wins() {
        let cli = Cli::try_parse_from(["barduck", "latest"]).unwrap();
        assert!(cli.config.is_none());
        let cli = Cli::try_parse_from(["barduck", "-c", "./custom.toml", "latest"]).unwrap();
        assert_eq!(cli.config, Some(std::path::PathBuf::from("./custom.toml")));
    }

    /// (spec: source-configuration — Config file defines sources)
    #[test]
    fn resolve_config_path_precedence() {
        use std::collections::HashMap;
        fn lookup_from(owned: HashMap<String, String>) -> impl FnMut(&str) -> Option<String> {
            move |name: &str| owned.get(name).cloned()
        }
        fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        }
        // Explicit flag wins over everything.
        let mut env = lookup_from(vars(&[
            ("BARDUCK_CONFIG", "/srv/shared.toml"),
            ("XDG_CONFIG_HOME", "/xdg"),
        ]));
        assert_eq!(
            resolve_config_path(Some(std::path::Path::new("./custom.toml")), &mut env).unwrap(),
            std::path::PathBuf::from("./custom.toml")
        );
        // Env beats the XDG default; the XDG file need not exist.
        let mut env = lookup_from(vars(&[("BARDUCK_CONFIG", "/srv/shared.toml")]));
        assert_eq!(
            resolve_config_path(None, &mut env).unwrap(),
            std::path::PathBuf::from("/srv/shared.toml")
        );
        // Empty env value is ignored (falls through to XDG logic).
        let dir = tempfile::tempdir().unwrap();
        let xdg = dir.path().join("xdg");
        let xdg_file = xdg.join("barduck").join("config.toml");
        std::fs::create_dir_all(xdg_file.parent().unwrap()).unwrap();
        std::fs::write(&xdg_file, "").unwrap();
        let mut env = lookup_from(vars(&[
            ("BARDUCK_CONFIG", "  "),
            ("XDG_CONFIG_HOME", xdg.to_str().unwrap()),
        ]));
        assert_eq!(resolve_config_path(None, &mut env).unwrap(), xdg_file);
        // XDG set and the file exists → XDG wins.
        let mut env = lookup_from(vars(&[("XDG_CONFIG_HOME", xdg.to_str().unwrap())]));
        assert_eq!(resolve_config_path(None, &mut env).unwrap(), xdg_file);
        // Unset/empty XDG → `$HOME/.config`.
        let home_file = dir
            .path()
            .join("home")
            .join(".config")
            .join("barduck")
            .join("config.toml");
        std::fs::create_dir_all(home_file.parent().unwrap()).unwrap();
        std::fs::write(&home_file, "").unwrap();
        let home = dir.path().join("home").to_str().unwrap().to_string();
        for xdg_value in [None, Some("")] {
            let home_clone = home.clone();
            let mut env = move |name: &str| match name {
                "XDG_CONFIG_HOME" => xdg_value.map(str::to_string),
                "HOME" => Some(home_clone.clone()),
                _ => None,
            };
            assert_eq!(resolve_config_path(None, &mut env).unwrap(), home_file);
        }
        // Missing XDG file, no CWD config → error names tried paths. Point
        // XDG/HOME at empty dirs; CWD fallback only applies when the file
        // exists, and this checkout ships none at its root.
        let empty_xdg = dir.path().join("empty-xdg");
        let empty_home = dir.path().join("empty-home");
        let mut env = lookup_from(vars(&[
            ("XDG_CONFIG_HOME", empty_xdg.to_str().unwrap()),
            ("HOME", empty_home.to_str().unwrap()),
        ]));
        let err = resolve_config_path(None, &mut env);
        if std::path::Path::new("config.toml").exists() {
            assert_eq!(
                err.unwrap(),
                std::path::PathBuf::from("config.toml"),
                "CWD fallback"
            );
        } else {
            let err = err.unwrap_err().to_string();
            assert!(err.contains("no config file found"), "{err}");
            assert!(err.contains("barduck/config.toml"), "{err}");
        }
    }
}
