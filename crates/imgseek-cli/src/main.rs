use std::path::PathBuf;

use clap::{Parser, Subcommand};
use imgseek::{run_index, IndexArgs};
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
    /// Index local directories or images
    Index(IndexArgs),

    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
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
