use anyhow::{Context as _, Result};
use clap::{Args, Parser, Subcommand};
use barduck::{
    cli_report, config, db::Db, query::Backend, run_daemon, tui,
};
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
    Latest(QueryArgs),
    /// Reading history for one source.
    History {
        source: String,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[command(flatten)]
        flags: QueryArgs,
    },
    /// Per-source health status.
    Health(QueryArgs),
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
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let config_path = std::fs::canonicalize(&cli.config)
        .with_context(|| format!("resolving config path {}", cli.config.display()))?;
    let config_dir = config_path
        .parent()
        .with_context(|| format!("config path {} has no parent directory", config_path.display()))?;
    std::env::set_current_dir(config_dir)
        .with_context(|| format!("changing directory to {}", config_dir.display()))?;
    let cfg = config::load(&config_path)?;

    match cli.cmd {
        Cmd::Daemon => multi_rt()?.block_on(run_daemon(cfg)),
        Cmd::Tui { daemon } => tui::run(&Backend::new(&cfg, daemon)?, &cfg),
        Cmd::Latest(a) => rt()?.block_on(cli_report::print_latest(&Backend::new(&cfg, a.daemon)?, &a.source, a.json)),
        Cmd::History { source, from, to, flags } => {
            let from = from.as_deref().map(cli_report::parse_time).transpose()?;
            let to = to.as_deref().map(cli_report::parse_time).transpose()?;
            rt()?.block_on(cli_report::print_history(
                &Backend::new(&cfg, flags.daemon)?,
                &source,
                from,
                to,
                flags.json,
            ))
        }
        Cmd::Health(a) => rt()?.block_on(cli_report::print_health(&Backend::new(&cfg, a.daemon)?, &cfg, &a.source, a.json)),
        Cmd::Logs { limit, flags } => rt()?.block_on(cli_report::print_logs(&Backend::new(&cfg, flags.daemon)?, limit, &flags.source, flags.json)),
        Cmd::Reset { yes } => {
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
            println!("Database reset: {}", cfg.database_path.display());
            Ok(())
        }
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
