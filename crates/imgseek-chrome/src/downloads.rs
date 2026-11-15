use crate::error::Result;
use crate::history::ChromeDownload;
use imgseek_core::{
    ahash_from_luma, compute_color_histogram, dhash_from_luma, normalize, pdq_from_luma,
    phash_from_luma, sniff_format, whash_from_luma, Config,
};
use imgseek_index::{
    DataSegmentWriter, DocumentRecord, IndexSegmentWriter, Manifest, MihEnsembleIndex,
    SegmentDescriptor,
};
use imgseek_store::{compute_sha256, BlobStore, Corpus, Database, IndexRecord};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DownloadIngestStats {
    pub total_downloads: usize,
    pub image_downloads_found: usize,
    pub existing_files_found: usize,
    pub missing_files_count: usize,
    pub ingested_count: usize,
    pub skipped_privacy_count: usize,
    pub failed_decode_count: usize,
}

/// Checks whether a Chrome download corresponds to an image file via MIME type or file extension.
pub fn is_image_download(download: &ChromeDownload) -> bool {
    if let Some(ref mime) = download.mime_type {
        if mime.to_ascii_lowercase().starts_with("image/") {
            return true;
        }
    }

    let p = Path::new(&download.target_path);
    if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        matches!(
            ext_lower.as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tiff" | "tif"
        )
    } else {
        false
    }
}

/// Checks if any URL or path matches a list of privacy exclusion patterns.
pub fn matches_exclude_patterns(url: &str, patterns: &[String]) -> bool {
    let url_lower = url.to_lowercase();
    for pattern in patterns {
        let p_lower = pattern.trim().to_lowercase();
        if p_lower.is_empty() {
            continue;
        }
        let clean = p_lower.trim_matches('*');
        if url_lower.contains(clean) {
            return true;
        }
    }
    false
}

struct PreparedDoc {
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
    download: ChromeDownload,
    file_bytes: Vec<u8>,
}

/// Ingests all valid downloaded images that still exist on disk into the database, blob store, and search segments.
#[allow(clippy::too_many_arguments)]
pub fn ingest_downloaded_images(
    downloads: &[ChromeDownload],
    db: &mut Database,
    blob_store: &BlobStore,
    config: &Config,
    corpus_name: &str,
    since: Option<i64>,
    exclude_patterns: &[String],
    index_dir: Option<&Path>,
) -> Result<DownloadIngestStats> {
    let mut stats = DownloadIngestStats::default();
    let mut prepared = Vec::new();

    for d in downloads {
        stats.total_downloads += 1;

        if let Some(since_ts) = since {
            if d.start_time > 0 && d.start_time < since_ts {
                continue;
            }
        }

        if !is_image_download(d) {
            continue;
        }
        stats.image_downloads_found += 1;

        let path = Path::new(&d.target_path);
        if !path.is_file() {
            stats.missing_files_count += 1;
            continue;
        }
        stats.existing_files_found += 1;

        // Check privacy exclusion patterns
        let check_url1 = d.tab_url.as_deref().unwrap_or("");
        let check_url2 = d.referrer.as_deref().unwrap_or("");
        let check_path = d.target_path.as_str();

        if matches_exclude_patterns(check_url1, exclude_patterns)
            || matches_exclude_patterns(check_url2, exclude_patterns)
            || matches_exclude_patterns(check_path, exclude_patterns)
        {
            stats.skipped_privacy_count += 1;
            continue;
        }

        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => {
                stats.failed_decode_count += 1;
                continue;
            }
        };

        let format = match sniff_format(&bytes) {
            Ok(fmt) => format!("{:?}", fmt).to_lowercase(),
            Err(_) => {
                stats.failed_decode_count += 1;
                continue;
            }
        };

        let norm = match normalize(&bytes, &config.normalize) {
            Ok(n) => n,
            Err(_) => {
                stats.failed_decode_count += 1;
                continue;
            }
        };

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

        prepared.push(PreparedDoc {
            sha256,
            format,
            width: norm.width,
            height: norm.height,
            ahash,
            dhash,
            phash,
            whash,
            pdq: pdq_hash.0,
            pdq_quality,
            chroma_bytes,
            download: d.clone(),
            file_bytes: bytes,
        });
    }

    if prepared.is_empty() {
        return Ok(stats);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    db.insert_corpus(&Corpus {
        corpus_id: corpus_name.to_string(),
        kind: "chrome".to_string(),
        root_uri: "chrome://history".to_string(),
        created_at: now,
    })?;

    let mut batch_records = Vec::with_capacity(prepared.len());
    for item in &prepared {
        let chroma_hex = blob_store.put(&item.chroma_bytes)?;
        let origin_uri = item
            .download
            .tab_url
            .clone()
            .or_else(|| item.download.referrer.clone())
            .unwrap_or_else(|| format!("file://{}", item.download.target_path));

        let title = Path::new(&item.download.target_path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());

        batch_records.push(IndexRecord {
            sha256: item.sha256,
            byte_size: item.file_bytes.len() as i64,
            format: item.format.clone(),
            width: item.width,
            height: item.height,
            ahash: item.ahash,
            dhash: item.dhash,
            phash: item.phash,
            whash: item.whash,
            chroma_blob_ref: chroma_hex,
            keypoints_ref: None,
            indexed_at: now,
            corpus_id: corpus_name.to_string(),
            origin_uri,
            title,
            discovered_at: item.download.start_time,
            file_size: Some(item.file_bytes.len() as i64),
            file_mtime: Some(item.download.start_time),
        });
    }

    let (_, sightings_added) = db.insert_batch(&batch_records)?;
    stats.ingested_count = sightings_added;

    // If an index directory is provided, append the newly indexed documents to a new search segment
    if let Some(idx_dir) = index_dir {
        let mut manifest = Manifest::load_or_create(idx_dir)?;
        let next_segment_id = manifest
            .segments
            .iter()
            .map(|s| s.segment_id)
            .max()
            .unwrap_or(0)
            + 1;

        let mut data_writer = DataSegmentWriter::create(idx_dir, next_segment_id)?;
        let mut ensemble = MihEnsembleIndex::new();
        let mut count_added = 0usize;

        for item in &prepared {
            if let Some(db_doc) = db.get_doc_by_sha256(&item.sha256)? {
                let chroma_ref = compute_sha256(&item.chroma_bytes);
                let rec = DocumentRecord {
                    doc_id: db_doc.doc_id as u32,
                    phash: item.phash,
                    whash: item.whash,
                    dhash: item.dhash,
                    ahash: item.ahash,
                    pdq: item.pdq,
                    pdq_quality: item.pdq_quality,
                    width: item.width as u16,
                    height: item.height as u16,
                    chroma_ref,
                };
                data_writer.add_record(&rec)?;
                ensemble.insert_record(&rec);
                count_added += 1;
            }
        }

        if count_added > 0 {
            let (_, data_crc, min_doc_id, max_doc_id) = data_writer.finish()?;
            let (_, idx_crc) =
                IndexSegmentWriter::write_index(idx_dir, next_segment_id, &ensemble)?;

            let tomb_filename = format!("seg_{:08}.tomb", next_segment_id);
            let tomb_path = idx_dir.join(&tomb_filename);
            if !tomb_path.exists() {
                let tomb_len = (max_doc_id.saturating_sub(min_doc_id) + 1).div_ceil(8) as usize;
                fs::write(&tomb_path, vec![0u8; tomb_len])?;
            }

            manifest.segments.push(SegmentDescriptor {
                segment_id: next_segment_id,
                doc_count: count_added as u32,
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
            manifest.save_atomic(idx_dir)?;
        }
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_image_download() {
        let mut d = ChromeDownload {
            id: 1,
            target_path: "/downloads/photo.jpg".into(),
            start_time: 1000,
            end_time: 1000,
            received_bytes: 500,
            total_bytes: 500,
            state: 1,
            mime_type: Some("image/jpeg".into()),
            tab_url: None,
            referrer: None,
        };
        assert!(is_image_download(&d));

        d.mime_type = Some("application/pdf".into());
        // Extension is still .jpg -> true
        assert!(is_image_download(&d));

        d.target_path = "/downloads/doc.pdf".into();
        assert!(!is_image_download(&d));
    }

    #[test]
    fn test_matches_exclude_patterns() {
        let patterns = vec!["*bank*".into(), "*private*".into()];
        assert!(matches_exclude_patterns(
            "https://mybank.com/login",
            &patterns
        ));
        assert!(matches_exclude_patterns(
            "https://example.com/private/photo.jpg",
            &patterns
        ));
        assert!(!matches_exclude_patterns(
            "https://example.com/public/photo.jpg",
            &patterns
        ));
    }

    #[test]
    fn test_ingest_downloaded_images_flow() {
        let tmp = std::env::temp_dir().join(format!(
            "chrome_dl_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("test.db");
        let blob_dir = tmp.join("blobs");
        let idx_dir = tmp.join("index");
        let download_dir = tmp.join("downloads");
        fs::create_dir_all(&download_dir).unwrap();

        let mut db = Database::open(&db_path).unwrap();
        let blob_store = BlobStore::new(&blob_dir).unwrap();
        let config = Config::default();

        // Create a 2x2 BMP image on disk
        let bmp_path = download_dir.join("sample.bmp");
        let bmp_bytes: [u8; 70] = [
            0x42, 0x4d, 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0,
            1, 0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0,
        ];
        fs::write(&bmp_path, bmp_bytes).unwrap();

        let downloads = vec![
            ChromeDownload {
                id: 1,
                target_path: bmp_path.to_str().unwrap().to_string(),
                start_time: 1_700_000_000,
                end_time: 1_700_000_000,
                received_bytes: 70,
                total_bytes: 70,
                state: 1,
                mime_type: Some("image/bmp".into()),
                tab_url: Some("https://example.com/page.html".into()),
                referrer: Some("https://example.com/".into()),
            },
            ChromeDownload {
                id: 2,
                target_path: download_dir
                    .join("nonexistent.jpg")
                    .to_str()
                    .unwrap()
                    .to_string(),
                start_time: 1_700_000_000,
                end_time: 1_700_000_000,
                received_bytes: 100,
                total_bytes: 100,
                state: 1,
                mime_type: Some("image/jpeg".into()),
                tab_url: None,
                referrer: None,
            },
            ChromeDownload {
                id: 3,
                target_path: download_dir.join("doc.pdf").to_str().unwrap().to_string(),
                start_time: 1_700_000_000,
                end_time: 1_700_000_000,
                received_bytes: 100,
                total_bytes: 100,
                state: 1,
                mime_type: Some("application/pdf".into()),
                tab_url: None,
                referrer: None,
            },
        ];

        let stats = ingest_downloaded_images(
            &downloads,
            &mut db,
            &blob_store,
            &config,
            "chrome_history",
            None,
            &[],
            Some(&idx_dir),
        )
        .unwrap();

        assert_eq!(stats.total_downloads, 3);
        assert_eq!(stats.image_downloads_found, 2);
        assert_eq!(stats.existing_files_found, 1);
        assert_eq!(stats.missing_files_count, 1);
        assert_eq!(stats.ingested_count, 1);

        // Verify sightings stored in db with exact provenance
        let doc = db
            .get_doc_by_sha256(&compute_sha256(&bmp_bytes))
            .unwrap()
            .expect("doc must exist");
        let sightings = db.get_sightings_by_doc_id(doc.doc_id).unwrap();
        assert_eq!(sightings.len(), 1);
        assert_eq!(sightings[0].origin_uri, "https://example.com/page.html");
        assert_eq!(sightings[0].corpus_id, "chrome_history");

        // Verify segment was created and has 1 doc
        let manifest = Manifest::load(&idx_dir).unwrap();
        assert_eq!(manifest.segments.len(), 1);
        assert_eq!(manifest.segments[0].doc_count, 1);

        let _ = fs::remove_dir_all(&tmp);
    }
}
