use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod db;
mod display;
mod server;

#[derive(Parser)]
#[command(name = "claude-meter", version, about = "Local OTLP collector for Claude Code telemetry")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the OTLP collector server
    Serve {
        #[arg(long, default_value = "4318")]
        port: u16,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Show today's usage summary
    Summary {
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Show all-time totals
    Totals {
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Show daily breakdown
    History {
        #[arg(long, default_value = "7")]
        days: u32,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Show usage by model
    ByModel {
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Show usage by session
    BySession {
        #[arg(long, default_value = "7")]
        days: u32,
        #[arg(long)]
        db: Option<PathBuf>,
    },
    /// Delete old data
    Purge {
        /// Duration: 30d, 6m, 1y
        #[arg(long)]
        older_than: String,
        #[arg(long)]
        db: Option<PathBuf>,
    },
}

fn default_db_path() -> PathBuf {
    let home = std::env::var("HOME").expect("HOME not set");
    PathBuf::from(home)
        .join(".claude-meter")
        .join("metrics.db")
}

fn parse_duration_to_secs(s: &str) -> anyhow::Result<i64> {
    let s = s.trim();
    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid duration number: {}", num_str))?;
    let secs = match unit {
        "d" => num * 86400,
        "m" => num * 86400 * 30,
        "y" => num * 86400 * 365,
        _ => anyhow::bail!("Unknown duration unit '{}'. Use d (days), m (months), y (years)", unit),
    };
    Ok(secs)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port, db } => {
            let db_path = db.unwrap_or_else(default_db_path);
            server::run(port, db_path).await?;
        }
        Commands::Summary { db } => {
            let db_path = db.unwrap_or_else(default_db_path);
            let conn = db::open(&db_path)?;
            let today_start = chrono::Local::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_local_timezone(chrono::Local)
                .unwrap()
                .timestamp();
            let usage = db::query_usage(&conn, today_start)?;
            display::show_summary(&usage, "Today", None);
        }
        Commands::Totals { db } => {
            let db_path = db.unwrap_or_else(default_db_path);
            let conn = db::open(&db_path)?;
            let earliest = db::earliest_timestamp(&conn, 0)?;
            let usage = db::query_usage(&conn, 0)?;
            display::show_summary(&usage, "All Time", earliest);
        }
        Commands::History { days, db } => {
            let db_path = db.unwrap_or_else(default_db_path);
            let conn = db::open(&db_path)?;
            let since = chrono::Local::now().timestamp() - (days as i64 * 86400);
            let rows = db::query_history(&conn, since)?;
            display::show_history(&rows);
        }
        Commands::ByModel { db } => {
            let db_path = db.unwrap_or_else(default_db_path);
            let conn = db::open(&db_path)?;
            let rows = db::query_by_model(&conn)?;
            display::show_by_model(&rows);
        }
        Commands::BySession { days, db } => {
            let db_path = db.unwrap_or_else(default_db_path);
            let conn = db::open(&db_path)?;
            let since = chrono::Local::now().timestamp() - (days as i64 * 86400);
            let rows = db::query_by_session(&conn, since)?;
            display::show_by_session(&rows);
        }
        Commands::Purge { older_than, db } => {
            let db_path = db.unwrap_or_else(default_db_path);
            let conn = db::open(&db_path)?;
            let secs = parse_duration_to_secs(&older_than)?;
            let cutoff = chrono::Local::now().timestamp() - secs;
            let count = db::count_before(&conn, cutoff)?;
            if count == 0 {
                println!("No rows to delete.");
                return Ok(());
            }
            println!("This will delete {} row(s) older than {}.", count, older_than);
            print!("Continue? [y/N] ");
            use std::io::Write;
            std::io::stdout().flush()?;
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            if input.trim().eq_ignore_ascii_case("y") {
                let deleted = db::purge(&conn, cutoff)?;
                println!("Deleted {} row(s).", deleted);
            } else {
                println!("Aborted.");
            }
        }
    }

    Ok(())
}
