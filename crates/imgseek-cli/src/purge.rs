use clap::Args;
use imgseek_core::Config;
use imgseek_index::{compact_segments, mark_tombstone};
use imgseek_store::{BlobStore, Database};

#[derive(Args, Debug, Clone)]
pub struct PurgeArgs {
    /// Name of the corpus to purge
    #[arg(long)]
    pub corpus: String,

    /// Skip compaction after database purge
    #[arg(long, default_value_t = false)]
    pub no_compact: bool,

    /// Skip vacuuming the SQLite database
    #[arg(long, default_value_t = false)]
    pub no_vacuum: bool,
}

pub fn run_purge(args: &PurgeArgs, config: &Config) -> anyhow::Result<()> {
    let index_dir = &config.index.dir;
    let blob_dir = &config.index.blob_dir;
    let db_path = index_dir.join("metadata.db");

    if !db_path.exists() {
        println!("No metadata database found at {}.", db_path.display());
        return Ok(());
    }

    println!("Purging corpus '{}'...", args.corpus);

    let mut db = Database::open(&db_path)?;
    let blob_store = BlobStore::new(blob_dir)?;

    let purge_report = db.purge_corpus(&args.corpus)?;
    println!(
        "Purged from database: {} sightings, {} documents.",
        purge_report.sightings_deleted, purge_report.docs_deleted
    );

    // Delete orphaned blobs
    let mut blobs_deleted = 0usize;
    for blob_ref in &purge_report.orphaned_blob_refs {
        if blob_store.delete(blob_ref).unwrap_or(false) {
            blobs_deleted += 1;
        }
    }
    println!("Deleted {} unreferenced blobs.", blobs_deleted);

    // Mark orphaned docs in search index tombstones
    let mut tombstones_marked = 0usize;
    if index_dir.join("manifest.json").exists() {
        for &doc_id in &purge_report.orphaned_doc_ids {
            if mark_tombstone(index_dir, doc_id as u32).unwrap_or(false) {
                tombstones_marked += 1;
            }
        }
        println!(
            "Marked {} document tombstones in search index.",
            tombstones_marked
        );

        if !args.no_compact && tombstones_marked > 0 {
            println!("Compacting search index segments to eliminate purged data...");
            let comp_res = compact_segments(index_dir, None)?;
            println!(
                "Compacted {} old segment(s): {} docs retained, {} docs physically purged.",
                comp_res.old_segments_count, comp_res.docs_retained, comp_res.docs_purged
            );
        }
    }

    if !args.no_vacuum {
        println!("Vacuuming SQLite database...");
        db.vacuum()?;
        println!("Vacuum complete.");
    }

    println!("Corpus '{}' successfully purged.", args.corpus);
    Ok(())
}
