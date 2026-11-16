use std::path::PathBuf;

use clap::{Parser, Subcommand};
use imgseek::{
    run_chrome_index, run_chrome_report, run_compact, run_index, run_query, run_verify,
    ChromeIndexArgs, ChromeReportArgs, CompactArgs, IndexArgs, QueryArgs, VerifyArgs,
};
use imgseek_core::Config;

#[derive(Parser)]
#[command(
    name = "imgseek",
    about = "Deterministic, explainable, local reverse image search engine"
)]
struct Cli {
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index local directories, images, or browser data
    Index(IndexArgs),

    /// Manage and inspect Chrome browser corpus
    Chrome {
        #[command(subcommand)]
        action: ChromeAction,
    },

    /// Query the index with an image
    Query(QueryArgs),

    /// Verify index segment integrity and checksums
    Verify(VerifyArgs),

    /// Compact index segments and purge tombstones
    Compact(CompactArgs),

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ChromeAction {
    /// Ingest from Google Chrome history, downloads, and disk cache
    Index(ChromeIndexArgs),

    /// Display statistics and report on indexed Chrome browsing data
    Report(ChromeReportArgs),
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Write a default config.toml file
    Init {
        /// Path where the configuration file should be written
        #[arg(short, long, default_value = "config.toml")]
        path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config = Config::load_or_default(cli.config.as_deref())?;

    match cli.command {
        Commands::Index(args) => {
            run_index(&args, &config)?;
        }
        Commands::Chrome { action } => match action {
            ChromeAction::Index(args) => {
                run_chrome_index(&args, &config)?;
            }
            ChromeAction::Report(args) => {
                run_chrome_report(&args, &config)?;
            }
        },
        Commands::Query(args) => {
            run_query(&args, &config)?;
        }
        Commands::Verify(args) => {
            run_verify(&args, &config)?;
        }
        Commands::Compact(args) => {
            run_compact(&args, &config)?;
        }
        Commands::Config { action } => match action {
            ConfigAction::Init { path } => {
                if path.exists() {
                    eprintln!("Configuration file already exists at {}", path.display());
                    return Ok(());
                }
                Config::save_default(&path)?;
                println!("Wrote default configuration to {}", path.display());
            }
        },
    }

    Ok(())
}
