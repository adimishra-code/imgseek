use clap::Args;
use imgseek_core::Config;
use imgseek_index::compact_segments;
use std::path::PathBuf;

#[derive(Args, Debug, Clone)]
pub struct CompactArgs {
    /// Custom index directory to compact
    #[arg(short, long)]
    pub index_dir: Option<PathBuf>,

    /// Specific segment IDs to merge
    #[arg(long)]
    pub segments: Vec<u64>,
}

pub fn run_compact(args: &CompactArgs, config: &Config) -> anyhow::Result<()> {
    let index_dir = args
        .index_dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(&config.index.dir));

    if !index_dir.exists() {
        eprintln!("Index directory does not exist: {}", index_dir.display());
        return Ok(());
    }

    let segment_ids = if args.segments.is_empty() {
        None
    } else {
        Some(args.segments.as_slice())
    };

    let result = compact_segments(&index_dir, segment_ids)?;

    if result.old_segments_count == 0 {
        println!("No segments required compaction.");
    } else {
        println!("=== imgseek Compaction Complete ===");
        println!("Old Segments Merged: {}", result.old_segments_count);
        println!("Documents Retained:  {}", result.docs_retained);
        println!("Documents Purged:    {}", result.docs_purged);
        if let Some(merged) = result.merged_segment {
            println!("New Segment ID:      {}", merged.segment_id);
            println!("New Segment Docs:    {}", merged.doc_count);
        } else {
            println!("All documents were purged; no new segment created.");
        }
    }

    Ok(())
}
