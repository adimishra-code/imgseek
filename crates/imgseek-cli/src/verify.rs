use clap::Args;
use imgseek_core::Config;
use imgseek_index::verify_index;
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct VerifyArgs {
    /// Custom index directory to verify
    #[arg(short, long)]
    pub index_dir: Option<PathBuf>,

    /// Emit verification report as JSON
    #[arg(long)]
    pub json: bool,
}

pub fn run_verify(args: &VerifyArgs, config: &Config) -> anyhow::Result<()> {
    let index_dir = args
        .index_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(&config.index.dir));

    if !index_dir.exists() {
        eprintln!("Index directory does not exist: {}", index_dir.display());
        return Ok(());
    }

    let report = verify_index(&index_dir)?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("=== imgseek Index Verification Report ===");
        println!("Index Directory: {}", report.index_dir.display());
        println!("Total Segments:  {}", report.total_segments);
        println!("Total Docs:      {}", report.total_docs);
        println!("Tombstones:      {}", report.total_tombstones);
        println!("Active Docs:     {}", report.active_docs);
        println!(
            "Overall Status:  {}",
            if report.all_segments_valid {
                "OK (VALID)"
            } else {
                "FAIL (CORRUPT)"
            }
        );

        for seg in &report.segments {
            println!(
                "  Segment {:08}: docs={}, tombstones={}, data_valid={}, idx_valid={}, tomb_valid={}, monotonic={}",
                seg.segment_id,
                seg.doc_count,
                seg.tombstone_count,
                seg.data_file_valid,
                seg.idx_file_valid,
                seg.tomb_file_valid,
                seg.monotonic_postings_valid
            );
            for err in &seg.errors {
                eprintln!("    [ERROR] {}", err);
            }
        }
    }

    if !report.all_segments_valid {
        anyhow::bail!("Verification failed: index contains corrupted segment(s)");
    }

    Ok(())
}
