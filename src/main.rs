use anyhow::{Context as _, Result};
use barduck::{cli_report, config, db::Db, query::Backend, run_daemon, tui};
use clap::{Args, Parser, Subcommand};
use std::io::Write as _;

#[derive(Parser)]
#[command(name = "barduck", version, about = "Single-binary home dashboard")]
struct Cli {
    /// Path to the TOML config file.
    #[arg(long, short, global = true, default_value = "config.toml")]
    config: std::path::PathBuf,

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
    /// Run one source once and print the parsed result without touching
    /// the database (debug).
    Fetch {
        /// Which source to run.
        #[arg(long, short = 's')]
        source: String,
        /// Output JSON instead of human-readable text.
        #[arg(long)]
        json: bool,
    },
}

/// Asks the user to type "yes" on stdin; any other input (including empty)
/// answers no. Returns false on a non-interactive/closed stdin rather than
/// erroring, so piping into this command can never accidentally confirm.
fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [yes/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer)? == 0 {
        return Ok(false);
    }
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let config_path = std::fs::canonicalize(&cli.config)
        .with_context(|| format!("resolving config path {}", cli.config.display()))?;
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
        )),
        Cmd::Reset { yes, json } => {
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
        Cmd::Fetch { source, json } => rt()?.block_on(cli_report::print_fetch(&cfg, &source, json)),
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
    #![allow(clippy::unwrap_used)]
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
    #[test]
    fn reset_accepts_json_flag() {
        let cli = Cli::try_parse_from(["barduck", "reset", "--json", "--yes"]).unwrap();
        assert!(matches!(cli.cmd, Cmd::Reset { json: true, .. }));
    }
}
