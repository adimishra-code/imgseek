use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use imgseek_core::Config;
use imgseek_query::{QueryEngine, QueryOptions, QueryResultItem};
use imgseek_store::{BlobStore, Database};

#[derive(Args, Debug, Clone)]
pub struct QueryArgs {
    /// Path to query image
    pub image_path: PathBuf,

    /// Maximum results to return
    #[arg(long, default_value_t = 20)]
    pub limit: usize,

    /// Minimum match score threshold
    #[arg(long)]
    pub min_score: Option<f64>,

    /// Output raw JSON with full explanation
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Restrict query to a named corpus
    #[arg(long)]
    pub corpus: Option<String>,
}

pub fn run_query(args: &QueryArgs, config: &Config) -> anyhow::Result<Vec<QueryResultItem>> {
    let start_time = Instant::now();

    let db_path = config.index.dir.join("metadata.db");
    if !db_path.is_file() {
        anyhow::bail!(
            "No index found at {}. Run `imgseek index <path>` first.",
            db_path.display()
        );
    }

    let db = Database::open(&db_path)?;
    let blob_store = BlobStore::new(&config.index.blob_dir)?;

    let min_score = args.min_score.unwrap_or(config.engine.min_exact_score);
    let options = QueryOptions {
        limit: args.limit,
        min_score,
        corpus: args.corpus.clone(),
    };

    let engine = QueryEngine::new(&db, &blob_store, config);
    let results = engine.query_file(&args.image_path, &options)?;

    let elapsed = start_time.elapsed().as_secs_f64();

    if args.json {
        let json = serde_json::to_string_pretty(&results)?;
        println!("{json}");
    } else {
        print_results_table(&results, elapsed, min_score);
    }

    Ok(results)
}

fn print_results_table(results: &[QueryResultItem], elapsed: f64, threshold: f64) {
    if results.is_empty() {
        println!(
            "No matches found (threshold: {:.3}) in {:.3}s.",
            threshold, elapsed
        );
        return;
    }

    println!(
        "{:<4} | {:<6} | {:<6} | {:<10} | {:<30} | Origin",
        "Rank", "Score", "Format", "Dimensions", "Title"
    );
    println!(
        "{:-<4}-+-{:-<6}-+-{:-<6}-+-{:-<10}-+-{:-<30}-+---------------------",
        "", "", "", "", ""
    );

    for r in results {
        let title = r.title.as_deref().unwrap_or("-");
        let dims = format!("{}x{}", r.width, r.height);
        println!(
            "{:<4} | {:<6.4} | {:<6} | {:<10} | {:<30} | {}",
            r.rank, r.score, r.format, dims, title, r.origin_uri
        );
    }

    println!("\nFound {} match(es) in {:.3}s.", results.len(), elapsed);
}
