use crate::error::Result;
use crate::manifest::{Manifest, SegmentDescriptor};
use crate::mih::MihEnsembleIndex;
use crate::segment::{DataSegmentReader, DataSegmentWriter, DocumentRecord, IndexSegmentWriter};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionResult {
    pub old_segments_count: usize,
    pub merged_segment: Option<SegmentDescriptor>,
    pub docs_retained: usize,
    pub docs_purged: usize,
}

/// Marks a document ID as tombstoned in its corresponding segment and updates manifest.
pub fn mark_tombstone<P: AsRef<Path>>(index_dir: P, doc_id: u32) -> Result<bool> {
    let dir = index_dir.as_ref();
    let mut manifest = Manifest::load(dir)?;

    let target_idx = manifest
        .segments
        .iter()
        .position(|s| doc_id >= s.min_doc_id && doc_id <= s.max_doc_id);

    let seg_idx = match target_idx {
        Some(idx) => idx,
        None => return Ok(false),
    };

    let seg = &mut manifest.segments[seg_idx];
    let tomb_path = dir.join(&seg.tomb_file);
    let mut tomb_bytes = if tomb_path.exists() {
        fs::read(&tomb_path)?
    } else {
        let needed_bytes = (seg.max_doc_id.saturating_sub(seg.min_doc_id) + 1).div_ceil(8) as usize;
        vec![0u8; needed_bytes]
    };

    let rel_idx = (doc_id - seg.min_doc_id) as usize;
    let byte_idx = rel_idx >> 3;
    let bit_idx = rel_idx & 7;

    if byte_idx >= tomb_bytes.len() {
        tomb_bytes.resize(byte_idx + 1, 0);
    }

    if (tomb_bytes[byte_idx] & (1 << bit_idx)) != 0 {
        // Already tombstoned
        return Ok(false);
    }

    tomb_bytes[byte_idx] |= 1 << bit_idx;
    fs::write(&tomb_path, &tomb_bytes)?;

    seg.tombstone_count += 1;
    manifest.generation += 1;
    manifest.save_atomic(dir)?;

    Ok(true)
}

/// Compacts multiple segments into a single new segment, filtering out any tombstoned documents.
pub fn compact_segments<P: AsRef<Path>>(
    index_dir: P,
    segment_ids: Option<&[u64]>,
) -> Result<CompactionResult> {
    let dir = index_dir.as_ref();
    let mut manifest = Manifest::load(dir)?;

    let target_segment_ids: HashSet<u64> = match segment_ids {
        Some(ids) => ids.iter().copied().collect(),
        None => {
            // By default, compact if there are at least 2 segments or if any segment has tombstones
            let should_compact = manifest.segments.len() >= 2
                || manifest.segments.iter().any(|s| s.tombstone_count > 0);
            if !should_compact {
                return Ok(CompactionResult {
                    old_segments_count: 0,
                    merged_segment: None,
                    docs_retained: 0,
                    docs_purged: 0,
                });
            }
            manifest.segments.iter().map(|s| s.segment_id).collect()
        }
    };

    if target_segment_ids.is_empty() {
        return Ok(CompactionResult {
            old_segments_count: 0,
            merged_segment: None,
            docs_retained: 0,
            docs_purged: 0,
        });
    }

    let segments_to_merge: Vec<SegmentDescriptor> = manifest
        .segments
        .iter()
        .filter(|s| target_segment_ids.contains(&s.segment_id))
        .cloned()
        .collect();

    if segments_to_merge.is_empty() {
        return Ok(CompactionResult {
            old_segments_count: 0,
            merged_segment: None,
            docs_retained: 0,
            docs_purged: 0,
        });
    }

    let mut retained_records: Vec<DocumentRecord> = Vec::new();
    let mut docs_purged = 0usize;

    for seg in &segments_to_merge {
        let tomb_path = dir.join(&seg.tomb_file);
        let tomb_bytes = if tomb_path.exists() {
            fs::read(&tomb_path)?
        } else {
            Vec::new()
        };

        let is_tombstoned = |doc_id: u32| -> bool {
            if doc_id < seg.min_doc_id || doc_id > seg.max_doc_id {
                return false;
            }
            let rel_idx = (doc_id - seg.min_doc_id) as usize;
            let byte_idx = rel_idx >> 3;
            let bit_idx = rel_idx & 7;
            if byte_idx < tomb_bytes.len() {
                (tomb_bytes[byte_idx] & (1 << bit_idx)) != 0
            } else {
                false
            }
        };

        let data_reader = DataSegmentReader::open(dir.join(&seg.data_file))?;
        for rec in data_reader.iter() {
            if is_tombstoned(rec.doc_id) {
                docs_purged += 1;
            } else {
                retained_records.push(rec);
            }
        }
    }

    // Sort retained records deterministically by doc_id ascending
    retained_records.sort_by_key(|r| r.doc_id);

    let next_segment_id = manifest
        .segments
        .iter()
        .map(|s| s.segment_id)
        .max()
        .unwrap_or(0)
        + 1;

    let merged_segment = if retained_records.is_empty() {
        None
    } else {
        let mut data_writer = DataSegmentWriter::create(dir, next_segment_id)?;
        let mut ensemble = MihEnsembleIndex::new();

        for rec in &retained_records {
            data_writer.add_record(rec)?;
            ensemble.insert_record(rec);
        }

        let (_, data_crc, min_doc_id, max_doc_id) = data_writer.finish()?;
        let (_, idx_crc) = IndexSegmentWriter::write_index(dir, next_segment_id, &ensemble)?;

        let tomb_filename = format!("seg_{:08}.tomb", next_segment_id);
        let tomb_path = dir.join(&tomb_filename);
        let tomb_len = (max_doc_id.saturating_sub(min_doc_id) + 1).div_ceil(8) as usize;
        fs::write(&tomb_path, vec![0u8; tomb_len])?;

        Some(SegmentDescriptor {
            segment_id: next_segment_id,
            doc_count: retained_records.len() as u32,
            tombstone_count: 0,
            idx_file: format!("seg_{:08}.idx", next_segment_id),
            data_file: format!("seg_{:08}.data", next_segment_id),
            tomb_file: tomb_filename,
            idx_checksum: idx_crc,
            data_checksum: data_crc,
            min_doc_id,
            max_doc_id,
        })
    };

    // Update manifest
    manifest
        .segments
        .retain(|s| !target_segment_ids.contains(&s.segment_id));

    if let Some(ref new_seg) = merged_segment {
        manifest.segments.push(new_seg.clone());
    }

    manifest.segments.sort_by_key(|s| s.segment_id);
    manifest.generation += 1;
    manifest.save_atomic(dir)?;

    // Safely unlink old segment files now that the atomic manifest points to the new state
    for seg in &segments_to_merge {
        let data_path = dir.join(&seg.data_file);
        let idx_path = dir.join(&seg.idx_file);
        let tomb_path = dir.join(&seg.tomb_file);

        let _ = fs::remove_file(data_path);
        let _ = fs::remove_file(idx_path);
        let _ = fs::remove_file(tomb_path);
    }

    Ok(CompactionResult {
        old_segments_count: segments_to_merge.len(),
        merged_segment,
        docs_retained: retained_records.len(),
        docs_purged,
    })
}
