use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use clap::Args;
use imgseek_chrome::{
    find_all_profiles, find_profile, ingest_cached_images, ingest_downloaded_images, HistoryReader,
    HistorySnapshot,
};
use imgseek_core::Config;
use imgseek_store::{BlobStore, Database};

#[derive(Args, Debug, Clone)]
pub struct ChromeIndexArgs {
    /// Custom Chrome user data directory (defaults to auto-detected system path)
    #[arg(long)]
    pub user_data_dir: Option<PathBuf>,

    /// Profile name to index [default: Default]
    #[arg(long, default_value = "Default")]
    pub profile: String,

    /// Index all detected Chrome profiles
    #[arg(long, default_value_t = false)]
    pub all_profiles: bool,

    /// Target corpus name
    #[arg(long, default_value = "chrome_history")]
    pub corpus: String,

    /// Only index items since date (YYYY-MM-DD or Unix timestamp)
    #[arg(long)]
    pub since: Option<String>,

    /// Skip disk cache ingestion
    #[arg(long, default_value_t = false)]
    pub no_cache: bool,

    /// Skip downloads ingestion
    #[arg(long, default_value_t = false)]
    pub no_downloads: bool,

    /// Comma-separated list of privacy URL/path exclusion patterns (e.g. *bank*,*private*)
    #[arg(long, value_delimiter = ',')]
    pub exclude_patterns: Vec<String>,

    /// Show verbose logs during indexing
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ChromeReportArgs {
    /// Target corpus name to report on [default: chrome_history]
    #[arg(long, default_value = "chrome_history")]
    pub corpus: String,
}

pub fn parse_since(since_str: &str) -> Option<i64> {
    if let Ok(ts) = since_str.parse::<i64>() {
        return Some(ts);
    }

    let parts: Vec<&str> = since_str.split('-').collect();
    if parts.len() == 3 {
        let year: i64 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        let day: u32 = parts[2].parse().ok()?;
        if (1..=12).contains(&month) && (1..=31).contains(&day) {
            let y = year - if month <= 2 { 1 } else { 0 };
            let era = if y >= 0 { y } else { y - 399 } / 400;
            let yoe = (y - era * 400) as u32;
            let mp = (month + if month > 2 { 0 } else { 12 }) as i64;
            let doy = (153 * (mp - 3) + 2) / 5 + day as i64 - 1;
            let doe = yoe as i64 * 365 + yoe as i64 / 4 - yoe as i64 / 100 + doy;
            let days = era * 146097 + doe - 719468;
            return Some(days * 86400);
        }
    }

    None
}

pub fn run_chrome_index(args: &ChromeIndexArgs, config: &Config) -> anyhow::Result<()> {
    let start_time = Instant::now();

    let index_dir = &config.index.dir;
    let blob_dir = &config.index.blob_dir;
    std::fs::create_dir_all(index_dir)?;
    std::fs::create_dir_all(blob_dir)?;

    let db_path = index_dir.join("metadata.db");
    let mut db = Database::open(&db_path)?;
    let blob_store = BlobStore::new(blob_dir)?;

    let since_ts = args.since.as_deref().and_then(parse_since);

    let profiles = if args.all_profiles {
        find_all_profiles(args.user_data_dir.as_deref())?
    } else {
        vec![find_profile(
            args.user_data_dir.as_deref(),
            Some(&args.profile),
        )?]
    };

    println!("Found {} Chrome profile(s) to process.", profiles.len());

    let mut total_dl_ingested = 0usize;
    let mut total_cache_ingested = 0usize;
    let mut total_pages_matched = 0usize;

    for profile in &profiles {
        println!(
            "\n--- Ingesting Chrome profile: {} ({}) ---",
            profile.name,
            profile.path.display()
        );

        let snap = match HistorySnapshot::create(&profile.path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "Warning: Could not snapshot history for profile {}: {}",
                    profile.name, e
                );
                continue;
            }
        };

        let history = match HistoryReader::read_file(snap.db_path()) {
            Ok(h) => h,
            Err(e) => {
                eprintln!(
                    "Warning: Could not read history database for profile {}: {}",
                    profile.name, e
                );
                continue;
            }
        };

        println!(
            "Loaded history: {} URLs, {} visits, {} downloads.",
            history.urls.len(),
            history.visits.len(),
            history.downloads.len()
        );

        if !args.no_downloads {
            println!("Ingesting downloaded images...");
            match ingest_downloaded_images(
                &history.downloads,
                &mut db,
                &blob_store,
                config,
                &args.corpus,
                since_ts,
                &args.exclude_patterns,
                Some(index_dir),
            ) {
                Ok(stats) => {
                    println!(
                        "Downloads: {} scanned, {} images, {} existing files, {} ingested, {} skipped by privacy.",
                        stats.total_downloads,
                        stats.image_downloads_found,
                        stats.existing_files_found,
                        stats.ingested_count,
                        stats.skipped_privacy_count
                    );
                    total_dl_ingested += stats.ingested_count;
                }
                Err(e) => eprintln!("Error ingesting downloads: {}", e),
            }
        }

        if !args.no_cache {
            let cache_dir = profile.cache_dir();
            println!("Ingesting disk cache from {}...", cache_dir.display());
            match ingest_cached_images(
                &cache_dir,
                &history,
                &mut db,
                &blob_store,
                config,
                &args.corpus,
                since_ts,
                &args.exclude_patterns,
                Some(index_dir),
            ) {
                Ok(stats) => {
                    println!(
                        "Cache: {} files, {} valid entries, {} images, {} ingested ({} matched pages, {} unmatched), {} skipped by privacy.",
                        stats.total_cache_files,
                        stats.valid_cache_entries,
                        stats.images_found,
                        stats.ingested_count,
                        stats.matched_pages_count,
                        stats.unmatched_pages_count,
                        stats.skipped_privacy_count
                    );
                    total_cache_ingested += stats.ingested_count;
                    total_pages_matched += stats.matched_pages_count;
                }
                Err(e) => eprintln!("Error ingesting cache: {}", e),
            }
        }
    }

    let elapsed = start_time.elapsed();
    println!("\n=== Chrome Index Complete ===");
    println!("Elapsed time: {:.2?}", elapsed);
    println!("Total downloads ingested: {}", total_dl_ingested);
    println!("Total cached images ingested: {}", total_cache_ingested);
    println!(
        "Total cache images matched to history pages: {}",
        total_pages_matched
    );

    Ok(())
}

pub fn run_chrome_report(args: &ChromeReportArgs, config: &Config) -> anyhow::Result<()> {
    let index_dir = &config.index.dir;
    let db_path = index_dir.join("metadata.db");
    if !db_path.exists() {
        println!("No metadata database found at {}.", db_path.display());
        return Ok(());
    }

    let db = Database::open(&db_path)?;
    let docs = db.get_docs_by_corpus(&args.corpus)?;

    println!("=== Chrome Corpus Report: '{}' ===", args.corpus);
    println!("Total unique image documents: {}", docs.len());

    let mut total_sightings = 0usize;
    let mut heuristic_page_matches = 0usize;
    let mut domain_counts: HashMap<String, usize> = HashMap::new();
    let mut format_counts: HashMap<String, usize> = HashMap::new();

    for doc in &docs {
        *format_counts.entry(doc.format.clone()).or_insert(0) += 1;
        let sightings = db.get_sightings_by_doc_id(doc.doc_id)?;
        for s in sightings {
            if s.corpus_id != args.corpus {
                continue;
            }
            total_sightings += 1;

            if let Some(ref extra) = s.extra_json {
                if extra.contains("\"page_match\":\"heuristic\"") {
                    heuristic_page_matches += 1;
                }
            }

            let host = imgseek_chrome::extract_host(&s.origin_uri);
            if !host.is_empty() {
                *domain_counts.entry(host.to_string()).or_insert(0) += 1;
            }
        }
    }

    println!("Total sightings: {}", total_sightings);
    println!(
        "Sightings with associated history pages: {}",
        heuristic_page_matches
    );

    println!("\nFormats:");
    for (fmt, count) in &format_counts {
        println!("  - {}: {}", fmt, count);
    }

    println!("\nTop Domains:");
    let mut sorted_domains: Vec<(String, usize)> = domain_counts.into_iter().collect();
    sorted_domains.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (domain, count) in sorted_domains.iter().take(10) {
        println!("  - {}: {}", domain, count);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_since() {
        assert_eq!(parse_since("1700000000"), Some(1700000000));
        let ts = parse_since("2026-11-15").unwrap();
        assert!(ts > 1_700_000_000);
        assert_eq!(parse_since("not-a-date"), None);
    }
}
