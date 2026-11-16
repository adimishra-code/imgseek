use crate::error::Result;
use crate::history::ChromeHistoryData;
use crate::simple_cache::{parse_cache_entry_bytes, parse_cache_item, ParsedCacheItem};
use imgseek_core::{
    ahash_from_luma, compute_color_histogram, dhash_from_luma, normalize, pdq_from_luma,
    phash_from_luma, whash_from_luma, Config,
};
use imgseek_index::{
    DataSegmentWriter, DocumentRecord, IndexSegmentWriter, Manifest, MihEnsembleIndex,
    SegmentDescriptor,
};
use imgseek_store::{compute_sha256, BlobStore, Corpus, Database, IndexRecord};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Result of associating a cached resource with a browsing history visit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PageMatchResult {
    pub page_url: Option<String>,
    pub page_title: Option<String>,
    pub visit_time: Option<i64>,
    pub delta_seconds: Option<i64>,
    pub top_frame_site: Option<String>,
    pub match_type: String,
}

impl PageMatchResult {
    pub fn none(top_frame_site: Option<String>) -> Self {
        Self {
            page_url: None,
            page_title: None,
            visit_time: None,
            delta_seconds: None,
            top_frame_site,
            match_type: "none".to_string(),
        }
    }

    pub fn to_extra_json(&self) -> Option<String> {
        if self.match_type == "none" {
            None
        } else {
            serde_json::to_string(&serde_json::json!({
                "page_match": self.match_type,
                "delta_seconds": self.delta_seconds,
                "top_frame_site": self.top_frame_site,
                "page_url": self.page_url,
            }))
            .ok()
        }
    }
}

/// Extracts a normalized domain/host string from a URL or site string.
pub fn extract_host(url_or_site: &str) -> &str {
    let s = url_or_site.trim();
    let s = if let Some(stripped) = s.strip_prefix("https://") {
        stripped
    } else if let Some(stripped) = s.strip_prefix("http://") {
        stripped
    } else {
        s
    };
    let s = s.split('/').next().unwrap_or(s);
    let s = s.split(':').next().unwrap_or(s);
    s.trim()
}

/// Matches a cached image entry to a history visit within a +/- 15 minute (900 seconds) window.
pub fn match_cache_entry_to_page(
    cache_item: &ParsedCacheItem,
    file_mtime: Option<i64>,
    history: &ChromeHistoryData,
) -> PageMatchResult {
    let entry_time = cache_item.headers.date_timestamp.or(file_mtime);

    let top_site = cache_item.top_frame_site.as_deref();
    let target_host = top_site
        .map(extract_host)
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| extract_host(&cache_item.resource_url));

    if target_host.is_empty() {
        return PageMatchResult::none(cache_item.top_frame_site.clone());
    }

    let url_map: HashMap<i64, &crate::history::ChromeUrl> =
        history.urls.iter().map(|u| (u.id, u)).collect();

    let mut best_match: Option<(&crate::history::ChromeUrl, i64, i64)> = None;

    for visit in &history.visits {
        if let Some(url_obj) = url_map.get(&visit.url_id) {
            let visit_host = extract_host(&url_obj.url);
            if visit_host.eq_ignore_ascii_case(target_host)
                || visit_host.ends_with(&format!(".{}", target_host))
                || target_host.ends_with(&format!(".{}", visit_host))
            {
                if let Some(e_time) = entry_time {
                    let delta = (visit.visit_time - e_time).abs();
                    if delta <= 900 {
                        let signed_delta = visit.visit_time - e_time;
                        match best_match {
                            None => best_match = Some((url_obj, visit.visit_time, signed_delta)),
                            Some((_, _, best_signed)) => {
                                if delta < best_signed.abs() {
                                    best_match = Some((url_obj, visit.visit_time, signed_delta));
                                }
                            }
                        }
                    }
                } else {
                    // If no timestamp available on cache entry, take the latest visit to this host
                    match best_match {
                        None => best_match = Some((url_obj, visit.visit_time, 0)),
                        Some((_, best_ts, _)) => {
                            if visit.visit_time > best_ts {
                                best_match = Some((url_obj, visit.visit_time, 0));
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some((matched_url, visit_time, delta)) = best_match {
        PageMatchResult {
            page_url: Some(matched_url.url.clone()),
            page_title: matched_url.title.clone(),
            visit_time: Some(visit_time),
            delta_seconds: Some(delta),
            top_frame_site: cache_item
                .top_frame_site
                .clone()
                .or_else(|| Some(format!("https://{}", target_host))),
            match_type: "heuristic".to_string(),
        }
    } else {
        PageMatchResult::none(cache_item.top_frame_site.clone())
    }
}

/// Statistics reported after ingesting Chrome disk cache images.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheIngestStats {
    pub total_cache_files: usize,
    pub valid_cache_entries: usize,
    pub images_found: usize,
    pub ingested_count: usize,
    pub skipped_privacy_count: usize,
    pub matched_pages_count: usize,
    pub unmatched_pages_count: usize,
    pub corrupt_entries_count: usize,
}

struct PreparedCacheDoc {
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
    resource_url: String,
    page_match: PageMatchResult,
    discovered_at: i64,
    file_bytes: Vec<u8>,
}

/// Ingests all valid cached images from a Chrome `Cache/Cache_Data/` directory.
#[allow(clippy::too_many_arguments)]
pub fn ingest_cached_images(
    cache_dir: &Path,
    history: &ChromeHistoryData,
    db: &mut Database,
    blob_store: &BlobStore,
    config: &Config,
    corpus_name: &str,
    since: Option<i64>,
    exclude_patterns: &[String],
    index_dir: Option<&Path>,
) -> Result<CacheIngestStats> {
    let mut stats = CacheIngestStats::default();

    if !cache_dir.is_dir() {
        return Ok(stats);
    }

    let mut prepared = Vec::new();

    let entries = fs::read_dir(cache_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with("_0") {
            continue;
        }

        stats.total_cache_files += 1;

        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                stats.corrupt_entries_count += 1;
                continue;
            }
        };

        let file_mtime = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        if let Some(since_ts) = since {
            if let Some(mtime) = file_mtime {
                if mtime < since_ts {
                    continue;
                }
            }
        }

        let file_bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(_) => {
                stats.corrupt_entries_count += 1;
                continue;
            }
        };

        let raw = match parse_cache_entry_bytes(&file_bytes) {
            Ok(r) => r,
            Err(_) => {
                stats.corrupt_entries_count += 1;
                continue;
            }
        };
        stats.valid_cache_entries += 1;

        let item = parse_cache_item(&raw);
        if item.image_format.is_none() {
            continue;
        }
        stats.images_found += 1;

        // Check privacy exclusion patterns
        let check1 = item.resource_url.as_str();
        let check2 = item.top_frame_site.as_deref().unwrap_or("");
        if crate::downloads::matches_exclude_patterns(check1, exclude_patterns)
            || crate::downloads::matches_exclude_patterns(check2, exclude_patterns)
        {
            stats.skipped_privacy_count += 1;
            continue;
        }

        let page_match = match_cache_entry_to_page(&item, file_mtime, history);
        if page_match.match_type == "none" {
            stats.unmatched_pages_count += 1;
        } else {
            stats.matched_pages_count += 1;
        }

        let norm = match normalize(&item.payload, &config.normalize) {
            Ok(n) => n,
            Err(_) => continue,
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

        let sha256 = compute_sha256(&item.payload);
        let discovered_at = page_match.visit_time.or(file_mtime).unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
        });

        prepared.push(PreparedCacheDoc {
            sha256,
            format: item.image_format.unwrap_or_else(|| "unknown".into()),
            width: norm.width,
            height: norm.height,
            ahash,
            dhash,
            phash,
            whash,
            pdq: pdq_hash.0,
            pdq_quality,
            chroma_bytes,
            resource_url: item.resource_url,
            page_match,
            discovered_at,
            file_bytes: item.payload,
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
        root_uri: "chrome://cache".to_string(),
        created_at: now,
    })?;

    let mut batch_records = Vec::with_capacity(prepared.len());
    for item in &prepared {
        let chroma_hex = blob_store.put(&item.chroma_bytes)?;
        let extra_json = item.page_match.to_extra_json();
        let title = item.page_match.page_title.clone();

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
            origin_uri: item.resource_url.clone(),
            title,
            discovered_at: item.discovered_at,
            file_size: Some(item.file_bytes.len() as i64),
            file_mtime: Some(item.discovered_at),
            extra_json,
        });
    }

    let (_, sightings_added) = db.insert_batch(&batch_records)?;
    stats.ingested_count = sightings_added;

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
    use crate::fake_profile::FakeChromeProfile;
    use crate::history::{ChromeUrl, ChromeVisit, HistoryReader};

    #[test]
    fn test_match_cache_entry_to_page_heuristic() {
        let history = ChromeHistoryData {
            urls: vec![
                ChromeUrl {
                    id: 1,
                    url: "https://example.com/blog/article1.html".into(),
                    title: Some("Example Article 1".into()),
                    visit_count: 1,
                    typed_count: 0,
                    last_visit_time: 1_700_000_100,
                },
                ChromeUrl {
                    id: 2,
                    url: "https://other.org/page.html".into(),
                    title: Some("Other Page".into()),
                    visit_count: 1,
                    typed_count: 0,
                    last_visit_time: 1_700_000_200,
                },
            ],
            visits: vec![
                ChromeVisit {
                    id: 1,
                    url_id: 1,
                    visit_time: 1_700_000_100,
                    from_visit: 0,
                    transition: 0,
                },
                ChromeVisit {
                    id: 2,
                    url_id: 2,
                    visit_time: 1_700_000_200,
                    from_visit: 0,
                    transition: 0,
                },
            ],
            downloads: vec![],
            download_chains: vec![],
            search_terms: vec![],
        };

        // Cache item from same domain 30 seconds after visit 1
        let cache_item = ParsedCacheItem {
            raw_key: "_dk_https://example.com https://example.com https://cdn.example.com/logo.png"
                .into(),
            resource_url: "https://cdn.example.com/logo.png".into(),
            top_frame_site: Some("https://example.com".into()),
            headers: crate::simple_cache::HttpHeaderInfo {
                content_type: Some("image/png".into()),
                date_str: Some("date".into()),
                date_timestamp: Some(1_700_000_130),
                last_modified_str: None,
                last_modified_timestamp: None,
            },
            image_format: Some("png".into()),
            payload: vec![],
        };

        let res = match_cache_entry_to_page(&cache_item, None, &history);
        assert_eq!(res.match_type, "heuristic");
        assert_eq!(
            res.page_url.as_deref(),
            Some("https://example.com/blog/article1.html")
        );
        assert_eq!(res.page_title.as_deref(), Some("Example Article 1"));
        assert_eq!(res.delta_seconds, Some(-30));
        assert!(res.to_extra_json().is_some());
    }

    #[test]
    fn test_match_cache_entry_outside_window_gives_none() {
        let history = ChromeHistoryData {
            urls: vec![ChromeUrl {
                id: 1,
                url: "https://example.com/page.html".into(),
                title: Some("Title".into()),
                visit_count: 1,
                typed_count: 0,
                last_visit_time: 1_700_000_000,
            }],
            visits: vec![ChromeVisit {
                id: 1,
                url_id: 1,
                visit_time: 1_700_000_000,
                from_visit: 0,
                transition: 0,
            }],
            downloads: vec![],
            download_chains: vec![],
            search_terms: vec![],
        };

        // 20 minutes (1200 seconds) after visit -> outside 900s window
        let cache_item = ParsedCacheItem {
            raw_key: "https://example.com/img.jpg".into(),
            resource_url: "https://example.com/img.jpg".into(),
            top_frame_site: Some("https://example.com".into()),
            headers: crate::simple_cache::HttpHeaderInfo {
                content_type: Some("image/jpeg".into()),
                date_str: None,
                date_timestamp: Some(1_700_001_200),
                last_modified_str: None,
                last_modified_timestamp: None,
            },
            image_format: Some("jpeg".into()),
            payload: vec![],
        };

        let res = match_cache_entry_to_page(&cache_item, None, &history);
        assert_eq!(res.match_type, "none");
        assert_eq!(res.page_url, None);
        assert_eq!(res.to_extra_json(), None);
    }

    #[test]
    fn test_ingest_cached_images_integration() {
        let tmp = std::env::temp_dir().join(format!(
            "cache_ingest_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();

        let db_path = tmp.join("metadata.db");
        let blob_dir = tmp.join("blobs");
        let idx_dir = tmp.join("index");
        let mut db = Database::open(&db_path).unwrap();
        let blob_store = BlobStore::new(&blob_dir).unwrap();
        let config = Config::default();

        let profile = FakeChromeProfile::create(&tmp, "Default").unwrap();
        let url_id = profile
            .add_url(
                "https://example.com/article",
                Some("Test Article"),
                1_700_000_000,
            )
            .unwrap();
        profile.add_visit(url_id, 1_700_000_000).unwrap();

        // 2x2 BMP
        let bmp_bytes: [u8; 70] = [
            0x42, 0x4d, 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0,
            1, 0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0,
        ];

        profile
            .add_cache_entry(
                "_dk_https://example.com https://example.com https://cdn.example.com/img.bmp",
                &bmp_bytes,
                Some("image/bmp"),
                Some("Sun, 15 Nov 2026 12:00:00 GMT"), // 1_794_744_000 or custom date
            )
            .unwrap();

        let snap = crate::snapshot::HistorySnapshot::create(&profile.profile_dir).unwrap();
        let history = HistoryReader::read_file(snap.db_path()).unwrap();

        let stats = ingest_cached_images(
            &profile.cache_dir,
            &history,
            &mut db,
            &blob_store,
            &config,
            "chrome_history",
            None,
            &[],
            Some(&idx_dir),
        )
        .unwrap();

        assert_eq!(stats.total_cache_files, 1);
        assert_eq!(stats.valid_cache_entries, 1);
        assert_eq!(stats.images_found, 1);
        assert_eq!(stats.ingested_count, 1);

        // Verify doc and sighting recorded
        let doc = db
            .get_doc_by_sha256(&compute_sha256(&bmp_bytes))
            .unwrap()
            .expect("doc exists");
        let sightings = db.get_sightings_by_doc_id(doc.doc_id).unwrap();
        assert_eq!(sightings.len(), 1);
        assert_eq!(sightings[0].origin_uri, "https://cdn.example.com/img.bmp");

        let _ = fs::remove_dir_all(&tmp);
    }
}
