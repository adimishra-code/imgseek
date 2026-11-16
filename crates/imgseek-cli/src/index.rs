use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::{Args, Subcommand};
use imgseek_core::{
    ahash_from_luma, compute_color_histogram, dhash_from_luma, normalize, pdq_from_luma,
    phash_from_luma, sniff_format, whash_from_luma, Config,
};
use imgseek_index::{
    DataSegmentWriter, DocumentRecord, IndexSegmentWriter, Manifest, MihEnsembleIndex,
    SegmentDescriptor,
};
use imgseek_store::{compute_sha256, BlobStore, Corpus, Database, IndexRecord};
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::chrome::{run_chrome_index, ChromeIndexArgs};

#[derive(Subcommand, Debug, Clone)]
pub enum IndexSubcommand {
    /// Ingest from Google Chrome history, downloads, and disk cache
    Chrome(ChromeIndexArgs),
}

#[derive(Args, Debug, Clone)]
pub struct IndexArgs {
    #[command(subcommand)]
    pub subcommand: Option<IndexSubcommand>,

    /// Paths to directories or image files to index
    #[arg(required_unless_present = "subcommand")]
    pub paths: Vec<PathBuf>,

    /// Target corpus name [default: local:<folder-name>]
    #[arg(long)]
    pub corpus: Option<String>,

    /// Walk subdirectories recursively
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub recursive: bool,

    /// Follow directory symlinks
    #[arg(long, default_value_t = false)]
    pub follow_symlinks: bool,

    /// Parallel worker threads [default: num_cpus]
    #[arg(long)]
    pub workers: Option<usize>,

    /// SQLite transaction batch size
    #[arg(long, default_value_t = 500)]
    pub batch_size: usize,

    /// Re-process files even if size + mtime match
    #[arg(long, default_value_t = false)]
    pub force_reindex: bool,

    /// Per-file verbose logs
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,
}

enum ProcessResult {
    Success(Box<ExtractedDoc>),
    Skipped(PathBuf, String),
    Error(PathBuf, String),
}

#[derive(Clone)]
struct ExtractedDoc {
    origin_uri: String,
    title: String,
    file_size: i64,
    file_mtime: i64,
    sha256: [u8; 32],
    format: String,
    width: u32,
    height: u32,
    ahash: u64,
    dhash: u64,
    phash: u64,
    whash: u64,
    pdq: [u8; 32],
    pdq_quality: u16,
    chroma_bytes: Vec<u8>,
}

pub fn run_index(args: &IndexArgs, config: &Config) -> anyhow::Result<()> {
    if let Some(IndexSubcommand::Chrome(ref chrome_args)) = args.subcommand {
        return run_chrome_index(chrome_args, config);
    }

    let start_time = Instant::now();

    let index_dir = &config.index.dir;
    let blob_dir = &config.index.blob_dir;
    std::fs::create_dir_all(index_dir)?;
    std::fs::create_dir_all(blob_dir)?;

    let db_path = index_dir.join("metadata.db");
    let mut db = Database::open(&db_path)?;
    let blob_store = BlobStore::new(blob_dir)?;

    let corpus_name = args.corpus.clone().unwrap_or_else(|| {
        let first_name = args
            .paths
            .first()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string());
        format!("local:{}", first_name)
    });

    let root_uri = args
        .paths
        .first()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    db.insert_corpus(&Corpus {
        corpus_id: corpus_name.clone(),
        kind: "local".to_string(),
        root_uri,
        created_at: now,
    })?;

    let existing_metadata = db.get_corpus_sightings_metadata(&corpus_name)?;
    let file_paths = collect_files(args)?;
    let total_candidates = file_paths.len();

    let mut files_to_process = Vec::new();
    let mut skipped_unchanged = 0usize;

    for path in file_paths {
        if !args.force_reindex {
            let origin_uri = path.to_string_lossy().to_string();
            if let Some(&(Some(stored_size), Some(stored_mtime))) =
                existing_metadata.get(&origin_uri)
            {
                if let Ok(metadata) = std::fs::metadata(&path) {
                    let file_size = metadata.len() as i64;
                    let file_mtime = metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    if file_size == stored_size && file_mtime == stored_mtime {
                        skipped_unchanged += 1;
                        if args.verbose {
                            println!("Skipped {}: unchanged", path.display());
                        }
                        continue;
                    }
                }
            }
        }
        files_to_process.push(path);
    }

    let pool = if let Some(workers) = args.workers {
        rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()?
    } else {
        rayon::ThreadPoolBuilder::new().build()?
    };

    let processed: Vec<ProcessResult> = pool.install(|| {
        files_to_process
            .par_iter()
            .map(|path| process_file(path, config, args.verbose))
            .collect()
    });

    let mut indexed_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;

    let mut batch_records = Vec::with_capacity(args.batch_size);
    let mut successful_docs = Vec::new();

    for res in processed {
        match res {
            ProcessResult::Success(doc) => {
                let chroma_ref = blob_store.put(&doc.chroma_bytes)?;

                batch_records.push(IndexRecord {
                    sha256: doc.sha256,
                    byte_size: doc.file_size,
                    format: doc.format.clone(),
                    width: doc.width,
                    height: doc.height,
                    ahash: doc.ahash,
                    dhash: doc.dhash,
                    phash: doc.phash,
                    whash: doc.whash,
                    chroma_blob_ref: chroma_ref,
                    keypoints_ref: None,
                    indexed_at: now,
                    corpus_id: corpus_name.clone(),
                    origin_uri: doc.origin_uri.clone(),
                    title: Some(doc.title.clone()),
                    discovered_at: now,
                    file_size: Some(doc.file_size),
                    file_mtime: Some(doc.file_mtime),
                    extra_json: None,
                });
                successful_docs.push(doc);

                if batch_records.len() >= args.batch_size {
                    let (_, sightings) = db.insert_batch(&batch_records)?;
                    indexed_count += sightings;
                    batch_records.clear();
                }
            }
            ProcessResult::Skipped(path, reason) => {
                skipped_count += 1;
                if args.verbose {
                    println!("Skipped {}: {}", path.display(), reason);
                }
            }
            ProcessResult::Error(path, err) => {
                failed_count += 1;
                eprintln!("Error {}: {}", path.display(), err);
            }
        }
    }

    if !batch_records.is_empty() {
        let (_, sightings) = db.insert_batch(&batch_records)?;
        indexed_count += sightings;
    }

    if indexed_count > 0 && !successful_docs.is_empty() {
        let mut manifest = Manifest::load_or_create(index_dir)?;
        let next_segment_id = manifest
            .segments
            .iter()
            .map(|s| s.segment_id)
            .max()
            .unwrap_or(0)
            + 1;

        let mut data_writer = DataSegmentWriter::create(index_dir, next_segment_id)?;
        let mut ensemble = MihEnsembleIndex::new();

        for doc in &successful_docs {
            if let Some(db_doc) = db.get_doc_by_sha256(&doc.sha256)? {
                let chroma_ref = compute_sha256(&doc.chroma_bytes);
                let rec = DocumentRecord {
                    doc_id: db_doc.doc_id as u32,
                    phash: doc.phash,
                    whash: doc.whash,
                    dhash: doc.dhash,
                    ahash: doc.ahash,
                    pdq: doc.pdq,
                    pdq_quality: doc.pdq_quality,
                    width: doc.width as u16,
                    height: doc.height as u16,
                    chroma_ref,
                };
                data_writer.add_record(&rec)?;
                ensemble.insert_record(&rec);
            }
        }

        let (_, data_crc, min_doc_id, max_doc_id) = data_writer.finish()?;
        let (_, idx_crc) = IndexSegmentWriter::write_index(index_dir, next_segment_id, &ensemble)?;

        let tomb_filename = format!("seg_{:08}.tomb", next_segment_id);
        let tomb_path = index_dir.join(&tomb_filename);
        if !tomb_path.exists() {
            std::fs::File::create(&tomb_path)?;
        }

        manifest.segments.push(SegmentDescriptor {
            segment_id: next_segment_id,
            doc_count: successful_docs.len() as u32,
            tombstone_count: 0,
            idx_file: format!("seg_{:08}.idx", next_segment_id),
            data_file: format!("seg_{:08}.data", next_segment_id),
            tomb_file: tomb_filename,
            idx_checksum: idx_crc,
            data_checksum: data_crc,
            min_doc_id,
            max_doc_id,
        });
        manifest.generation += 1;
        manifest.save_atomic(index_dir)?;
    }

    let elapsed = start_time.elapsed().as_secs_f64();
    let throughput = if elapsed > 0.0 {
        indexed_count as f64 / elapsed
    } else {
        indexed_count as f64
    };
    let total_skipped = skipped_unchanged + skipped_count;

    println!(
        "Indexed {} images (found: {}, skipped: {} [{} unchanged], failed: {}) in {:.2}s ({:.1} images/s).",
        indexed_count, total_candidates, total_skipped, skipped_unchanged, failed_count, elapsed, throughput
    );

    Ok(())
}

fn collect_files(args: &IndexArgs) -> anyhow::Result<Vec<PathBuf>> {
    let mut file_paths = Vec::new();

    for path in &args.paths {
        if path.is_file() {
            if !is_hidden(path) {
                file_paths.push(path.clone());
            }
            continue;
        }

        if path.is_dir() {
            let mut walker = WalkDir::new(path).follow_links(args.follow_symlinks);
            if !args.recursive {
                walker = walker.max_depth(1);
            }

            for entry in walker.into_iter().filter_entry(|e| !is_hidden(e.path())) {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) => {
                        if args.verbose {
                            eprintln!("Error accessing file: {}", e);
                        }
                        continue;
                    }
                };

                if entry.file_type().is_file() {
                    file_paths.push(entry.into_path());
                }
            }
        }
    }

    Ok(file_paths)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().starts_with('.'))
        .unwrap_or(false)
}

fn process_file(path: &Path, config: &Config, _verbose: bool) -> ProcessResult {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return ProcessResult::Error(path.to_path_buf(), e.to_string()),
    };

    let format = match sniff_format(&bytes) {
        Ok(f) => f,
        Err(_) => {
            return ProcessResult::Skipped(
                path.to_path_buf(),
                "unsupported or non-image format".to_string(),
            )
        }
    };

    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return ProcessResult::Error(path.to_path_buf(), e.to_string()),
    };
    let file_size = metadata.len() as i64;
    let file_mtime = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let norm = match normalize(&bytes, &config.normalize) {
        Ok(n) => n,
        Err(e) => return ProcessResult::Error(path.to_path_buf(), e.to_string()),
    };

    if norm.width < config.ingest.min_dimension || norm.height < config.ingest.min_dimension {
        return ProcessResult::Skipped(
            path.to_path_buf(),
            format!(
                "dimensions {}x{} below min_dimension",
                norm.width, norm.height
            ),
        );
    }

    let ahash = ahash_from_luma(
        norm.analysis.width,
        norm.analysis.height,
        &norm.analysis.luma,
    );
    let dhash = dhash_from_luma(
        norm.analysis.width,
        norm.analysis.height,
        &norm.analysis.luma,
    );
    let phash = phash_from_luma(
        norm.analysis.width,
        norm.analysis.height,
        &norm.analysis.luma,
    );
    let whash = whash_from_luma(
        norm.analysis.width,
        norm.analysis.height,
        &norm.analysis.luma,
    );

    let (pdq_hash, pdq_quality) = pdq_from_luma(
        norm.analysis.width,
        norm.analysis.height,
        &norm.analysis.luma,
    );

    let chroma = compute_color_histogram(&norm.analysis.rgb);
    let mut chroma_bytes = Vec::with_capacity(256 * 4);
    for bin in &chroma {
        chroma_bytes.extend_from_slice(&bin.to_le_bytes());
    }

    let sha256 = compute_sha256(&bytes);
    let title = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let origin_uri = path.to_string_lossy().to_string();

    ProcessResult::Success(Box::new(ExtractedDoc {
        origin_uri,
        title,
        file_size,
        file_mtime,
        sha256,
        format: format.as_str().to_string(),
        width: norm.width,
        height: norm.height,
        ahash,
        dhash,
        phash,
        whash,
        pdq: pdq_hash.0,
        pdq_quality,
        chroma_bytes,
    }))
}
