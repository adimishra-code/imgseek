use crate::compression::decode_postings;
use crate::error::{IndexError, Result};
use crate::manifest::{Manifest, SegmentDescriptor, CURRENT_FORMAT_VERSION};
use crate::segment::{
    ChunkTableDescriptor, DATA_MAGIC, FOOTER_LEN, FOOTER_MAGIC, HEADER_LEN, IDX_MAGIC, RECORD_LEN,
};
use crc32fast::Hasher;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentVerification {
    pub segment_id: u64,
    pub doc_count: u32,
    pub tombstone_count: u32,
    pub data_file_valid: bool,
    pub idx_file_valid: bool,
    pub tomb_file_valid: bool,
    pub monotonic_postings_valid: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationReport {
    pub index_dir: PathBuf,
    pub total_segments: usize,
    pub total_docs: u64,
    pub total_tombstones: u64,
    pub active_docs: u64,
    pub all_segments_valid: bool,
    pub segments: Vec<SegmentVerification>,
}

pub fn verify_index<P: AsRef<Path>>(index_dir: P) -> Result<VerificationReport> {
    let dir = index_dir.as_ref();
    let manifest = Manifest::load(dir)?;

    if manifest.format_version != CURRENT_FORMAT_VERSION {
        return Err(IndexError::VersionMismatch {
            expected: CURRENT_FORMAT_VERSION,
            actual: manifest.format_version,
        });
    }

    let mut total_docs = 0u64;
    let mut total_tombstones = 0u64;
    let mut all_segments_valid = true;
    let mut segment_reports = Vec::with_capacity(manifest.segments.len());

    for desc in &manifest.segments {
        let seg_report = verify_segment(dir, desc)?;
        total_docs += seg_report.doc_count as u64;
        total_tombstones += seg_report.tombstone_count as u64;

        if !seg_report.errors.is_empty()
            || !seg_report.data_file_valid
            || !seg_report.idx_file_valid
            || !seg_report.tomb_file_valid
            || !seg_report.monotonic_postings_valid
        {
            all_segments_valid = false;
        }

        segment_reports.push(seg_report);
    }

    let active_docs = total_docs.saturating_sub(total_tombstones);

    Ok(VerificationReport {
        index_dir: dir.to_path_buf(),
        total_segments: manifest.segments.len(),
        total_docs,
        total_tombstones,
        active_docs,
        all_segments_valid,
        segments: segment_reports,
    })
}

pub fn verify_segment<P: AsRef<Path>>(
    index_dir: P,
    desc: &SegmentDescriptor,
) -> Result<SegmentVerification> {
    let dir = index_dir.as_ref();
    let mut errors = Vec::new();

    let mut data_file_valid = true;
    let mut idx_file_valid = true;
    let mut tomb_file_valid = true;
    let mut monotonic_postings_valid = true;

    // 1. Verify Data Segment
    let data_path = dir.join(&desc.data_file);
    if !data_path.exists() {
        errors.push(format!("data file missing: {:?}", data_path));
        data_file_valid = false;
    } else {
        match fs::read(&data_path) {
            Err(e) => {
                errors.push(format!("failed to read data file: {}", e));
                data_file_valid = false;
            }
            Ok(data_bytes) => {
                if data_bytes.len() < HEADER_LEN + FOOTER_LEN {
                    errors.push(format!("data file too small: {} bytes", data_bytes.len()));
                    data_file_valid = false;
                } else {
                    // Header check
                    let magic = &data_bytes[0..8];
                    if magic != DATA_MAGIC {
                        errors.push(format!("data header magic invalid: {:?}", magic));
                        data_file_valid = false;
                    }

                    let version = u32::from_le_bytes(data_bytes[8..12].try_into().unwrap());
                    if version != 1 {
                        errors.push(format!("data format version invalid: {}", version));
                        data_file_valid = false;
                    }

                    let seg_id = u64::from_le_bytes(data_bytes[12..20].try_into().unwrap());
                    if seg_id != desc.segment_id {
                        errors.push(format!(
                            "data segment_id mismatch: expected {} found {}",
                            desc.segment_id, seg_id
                        ));
                        data_file_valid = false;
                    }

                    let doc_count = u32::from_le_bytes(data_bytes[20..24].try_into().unwrap());
                    if doc_count != desc.doc_count {
                        errors.push(format!(
                            "data doc_count mismatch: expected {} found {}",
                            desc.doc_count, doc_count
                        ));
                        data_file_valid = false;
                    }

                    // Payload bounds
                    let payload_len = data_bytes.len() - HEADER_LEN - FOOTER_LEN;
                    let expected_payload = desc.doc_count as usize * RECORD_LEN;
                    if payload_len != expected_payload {
                        errors.push(format!(
                            "data payload length mismatch: expected {} found {}",
                            expected_payload, payload_len
                        ));
                        data_file_valid = false;
                    }

                    // Footer check
                    let footer_offset = data_bytes.len() - FOOTER_LEN;
                    let footer_payload_len = u64::from_le_bytes(
                        data_bytes[footer_offset..footer_offset + 8]
                            .try_into()
                            .unwrap(),
                    );
                    let stored_crc = u32::from_le_bytes(
                        data_bytes[footer_offset + 8..footer_offset + 12]
                            .try_into()
                            .unwrap(),
                    );
                    let footer_magic = &data_bytes[footer_offset + 12..footer_offset + 16];

                    if footer_magic != FOOTER_MAGIC {
                        errors.push("data footer magic invalid".into());
                        data_file_valid = false;
                    }
                    if footer_payload_len != payload_len as u64 {
                        errors.push(format!(
                            "data footer payload_len {} != actual {}",
                            footer_payload_len, payload_len
                        ));
                        data_file_valid = false;
                    }

                    // CRC check
                    let mut hasher = Hasher::new();
                    hasher.update(&data_bytes[0..footer_offset]);
                    let computed_crc = hasher.finalize();

                    if computed_crc != stored_crc {
                        errors.push(format!(
                            "data CRC mismatch: computed {:08x} != footer {:08x}",
                            computed_crc, stored_crc
                        ));
                        data_file_valid = false;
                    }
                    if computed_crc != desc.data_checksum {
                        errors.push(format!(
                            "data CRC mismatch with manifest: computed {:08x} != manifest {:08x}",
                            computed_crc, desc.data_checksum
                        ));
                        data_file_valid = false;
                    }

                    // Validate document record IDs
                    for i in 0..desc.doc_count as usize {
                        let rec_start = HEADER_LEN + i * RECORD_LEN;
                        let doc_id = u32::from_le_bytes(
                            data_bytes[rec_start..rec_start + 4].try_into().unwrap(),
                        );
                        if doc_id < desc.min_doc_id || doc_id > desc.max_doc_id {
                            errors.push(format!(
                                "doc_id {} out of range [{}, {}]",
                                doc_id, desc.min_doc_id, desc.max_doc_id
                            ));
                            data_file_valid = false;
                            break;
                        }
                    }
                }
            }
        }
    }

    // 2. Verify Index Segment
    let idx_path = dir.join(&desc.idx_file);
    if !idx_path.exists() {
        errors.push(format!("idx file missing: {:?}", idx_path));
        idx_file_valid = false;
    } else {
        match fs::read(&idx_path) {
            Err(e) => {
                errors.push(format!("failed to read idx file: {}", e));
                idx_file_valid = false;
            }
            Ok(idx_bytes) => {
                if idx_bytes.len() < HEADER_LEN + FOOTER_LEN {
                    errors.push(format!("idx file too small: {} bytes", idx_bytes.len()));
                    idx_file_valid = false;
                } else {
                    // Header check
                    let magic = &idx_bytes[0..8];
                    if magic != IDX_MAGIC {
                        errors.push(format!("idx header magic invalid: {:?}", magic));
                        idx_file_valid = false;
                    }

                    let version = u32::from_le_bytes(idx_bytes[8..12].try_into().unwrap());
                    if version != 1 {
                        errors.push(format!("idx format version invalid: {}", version));
                        idx_file_valid = false;
                    }

                    let seg_id = u64::from_le_bytes(idx_bytes[12..20].try_into().unwrap());
                    if seg_id != desc.segment_id {
                        errors.push(format!(
                            "idx segment_id mismatch: expected {} found {}",
                            desc.segment_id, seg_id
                        ));
                        idx_file_valid = false;
                    }

                    let doc_count = u32::from_le_bytes(idx_bytes[20..24].try_into().unwrap());
                    if doc_count != desc.doc_count {
                        errors.push(format!(
                            "idx doc_count mismatch: expected {} found {}",
                            desc.doc_count, doc_count
                        ));
                        idx_file_valid = false;
                    }

                    // Footer check
                    let footer_offset = idx_bytes.len() - FOOTER_LEN;
                    let stored_crc = u32::from_le_bytes(
                        idx_bytes[footer_offset + 8..footer_offset + 12]
                            .try_into()
                            .unwrap(),
                    );
                    let footer_magic = &idx_bytes[footer_offset + 12..footer_offset + 16];

                    if footer_magic != FOOTER_MAGIC {
                        errors.push("idx footer magic invalid".into());
                        idx_file_valid = false;
                    }

                    // CRC check
                    let mut hasher = Hasher::new();
                    hasher.update(&idx_bytes[0..footer_offset]);
                    let computed_crc = hasher.finalize();

                    if computed_crc != stored_crc {
                        errors.push(format!(
                            "idx CRC mismatch: computed {:08x} != footer {:08x}",
                            computed_crc, stored_crc
                        ));
                        idx_file_valid = false;
                    }
                    if computed_crc != desc.idx_checksum {
                        errors.push(format!(
                            "idx CRC mismatch with manifest: computed {:08x} != manifest {:08x}",
                            computed_crc, desc.idx_checksum
                        ));
                        idx_file_valid = false;
                    }

                    // Check chunk table directory and verify monotonic postings
                    let payload = &idx_bytes[HEADER_LEN..footer_offset];
                    if payload.len() < 4 {
                        errors.push("idx payload too short for directory".into());
                        idx_file_valid = false;
                    } else {
                        let num_tables =
                            u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
                        let dir_entry_size = 20usize;
                        let dir_end = 4 + num_tables * dir_entry_size;

                        if payload.len() < dir_end {
                            errors.push("idx directory exceeds payload length".into());
                            idx_file_valid = false;
                        } else {
                            let mut descriptors = Vec::with_capacity(num_tables);
                            for i in 0..num_tables {
                                let entry_start = 4 + i * dir_entry_size;
                                let raw_algo = payload[entry_start];
                                let chunk_idx = payload[entry_start + 1];
                                let offset = u64::from_le_bytes(
                                    payload[entry_start + 4..entry_start + 12]
                                        .try_into()
                                        .unwrap(),
                                ) as usize;
                                let length = u64::from_le_bytes(
                                    payload[entry_start + 12..entry_start + 20]
                                        .try_into()
                                        .unwrap(),
                                ) as usize;

                                if let Some(algo_id) = crate::mih::HashAlgoId::from_u8(raw_algo) {
                                    if offset + length > payload.len() {
                                        errors.push(format!(
                                            "chunk table {} bounds out of payload",
                                            i
                                        ));
                                        idx_file_valid = false;
                                    } else {
                                        descriptors.push(ChunkTableDescriptor {
                                            algo_id,
                                            chunk_idx,
                                            offset,
                                            length,
                                        });
                                    }
                                } else {
                                    errors.push(format!("unknown algo id: {}", raw_algo));
                                    idx_file_valid = false;
                                }
                            }

                            // Verify postings monotonicity and bounds
                            'check_tables: for (t_idx, desc_table) in descriptors.iter().enumerate()
                            {
                                let table_slice = &payload
                                    [desc_table.offset..desc_table.offset + desc_table.length];
                                let bucket_dir_len = 65536 * 8;
                                if table_slice.len() < bucket_dir_len {
                                    errors.push(format!(
                                        "table {} slice too small for buckets",
                                        t_idx
                                    ));
                                    idx_file_valid = false;
                                    break;
                                }

                                let postings_stream = &table_slice[bucket_dir_len..];

                                for b in 0..65536 {
                                    let b_start = b * 8;
                                    let offset = u32::from_le_bytes(
                                        table_slice[b_start..b_start + 4].try_into().unwrap(),
                                    ) as usize;
                                    let len = u32::from_le_bytes(
                                        table_slice[b_start + 4..b_start + 8].try_into().unwrap(),
                                    ) as usize;

                                    if len > 0 {
                                        if offset + len > postings_stream.len() {
                                            errors.push(format!(
                                                "table {} bucket {} offset {} len {} exceeds stream {}",
                                                t_idx,
                                                b,
                                                offset,
                                                len,
                                                postings_stream.len()
                                            ));
                                            monotonic_postings_valid = false;
                                            break 'check_tables;
                                        }

                                        let slice = &postings_stream[offset..offset + len];
                                        let mut cursor = 0;
                                        let mut decoded = Vec::new();
                                        if let Err(e) =
                                            decode_postings(slice, &mut cursor, &mut decoded)
                                        {
                                            errors.push(format!(
                                                "failed to decode postings table {} bucket {}: {}",
                                                t_idx, b, e
                                            ));
                                            monotonic_postings_valid = false;
                                            break 'check_tables;
                                        }

                                        if cursor != len {
                                            errors.push(format!(
                                                "decoded postings consumed {} bytes != expected {}",
                                                cursor, len
                                            ));
                                            monotonic_postings_valid = false;
                                            break 'check_tables;
                                        }

                                        for window in decoded.windows(2) {
                                            if window[0] >= window[1] {
                                                errors.push(format!(
                                                    "postings not strictly monotonic: {} >= {}",
                                                    window[0], window[1]
                                                ));
                                                monotonic_postings_valid = false;
                                                break 'check_tables;
                                            }
                                        }

                                        for doc_id in &decoded {
                                            if *doc_id < desc.min_doc_id
                                                || *doc_id > desc.max_doc_id
                                            {
                                                errors.push(format!(
                                                    "posting doc_id {} outside range [{}, {}]",
                                                    doc_id, desc.min_doc_id, desc.max_doc_id
                                                ));
                                                monotonic_postings_valid = false;
                                                break 'check_tables;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Verify Tombstone Bitset
    let tomb_path = dir.join(&desc.tomb_file);
    if !tomb_path.exists() {
        errors.push(format!("tomb file missing: {:?}", tomb_path));
        tomb_file_valid = false;
    } else {
        match fs::read(&tomb_path) {
            Err(e) => {
                errors.push(format!("failed to read tomb file: {}", e));
                tomb_file_valid = false;
            }
            Ok(tomb_bytes) => {
                let mut actual_tombstones = 0u32;
                for b in &tomb_bytes {
                    actual_tombstones += b.count_ones();
                }

                if actual_tombstones != desc.tombstone_count {
                    errors.push(format!(
                        "tombstone count mismatch: actual {} != manifest {}",
                        actual_tombstones, desc.tombstone_count
                    ));
                    tomb_file_valid = false;
                }
            }
        }
    }

    Ok(SegmentVerification {
        segment_id: desc.segment_id,
        doc_count: desc.doc_count,
        tombstone_count: desc.tombstone_count,
        data_file_valid,
        idx_file_valid,
        tomb_file_valid,
        monotonic_postings_valid,
        errors,
    })
}
