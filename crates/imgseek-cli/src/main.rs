use clap::{Parser, Subcommand};
use imgseek_core::Config;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "imgseek",
    about = "Deterministic, explainable, local reverse image search engine"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
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

    match cli.command {
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
