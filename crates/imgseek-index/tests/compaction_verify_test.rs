use imgseek_index::{
    compact_segments, mark_tombstone, verify_index, DataSegmentWriter, DocumentRecord,
    IndexSegmentWriter, Manifest, MihEnsembleIndex, SegmentDescriptor,
};
use std::path::PathBuf;

fn make_temp_dir(prefix: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "imgseek_{}_{}_{}",
        prefix,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn dummy_record(doc_id: u32) -> DocumentRecord {
    DocumentRecord {
        doc_id,
        phash: 0x0123_4567_89ab_cdef ^ (doc_id as u64),
        whash: 0xfedc_ba98_7654_3210 ^ (doc_id as u64),
        dhash: 0x1122_3344_5566_7788 ^ (doc_id as u64),
        ahash: 0x8877_6655_4433_2211 ^ (doc_id as u64),
        pdq: [
            (doc_id & 0xff) as u8,
            ((doc_id >> 8) & 0xff) as u8,
            0x42,
            0x24,
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
            10,
            11,
            12,
            13,
            14,
            15,
            16,
            17,
            18,
            19,
            20,
            21,
            22,
            23,
            24,
            25,
            26,
            27,
            28,
        ],
        pdq_quality: 90,
        width: 640,
        height: 480,
        chroma_ref: [0x55; 32],
    }
}

fn write_test_segment(
    dir: &std::path::Path,
    segment_id: u64,
    doc_ids: &[u32],
) -> SegmentDescriptor {
    let mut data_writer = DataSegmentWriter::create(dir, segment_id).unwrap();
    let mut ensemble = MihEnsembleIndex::new();

    for &id in doc_ids {
        let rec = dummy_record(id);
        data_writer.add_record(&rec).unwrap();
        ensemble.insert_record(&rec);
    }

    let (_, data_crc, min_doc_id, max_doc_id) = data_writer.finish().unwrap();
    let (_, idx_crc) = IndexSegmentWriter::write_index(dir, segment_id, &ensemble).unwrap();

    let tomb_filename = format!("seg_{:08}.tomb", segment_id);
    let tomb_path = dir.join(&tomb_filename);
    let tomb_len = (max_doc_id.saturating_sub(min_doc_id) + 1).div_ceil(8) as usize;
    std::fs::write(&tomb_path, vec![0u8; tomb_len]).unwrap();

    SegmentDescriptor {
        segment_id,
        doc_count: doc_ids.len() as u32,
        tombstone_count: 0,
        idx_file: format!("seg_{:08}.idx", segment_id),
        data_file: format!("seg_{:08}.data", segment_id),
        tomb_file: tomb_filename,
        idx_checksum: idx_crc,
        data_checksum: data_crc,
        min_doc_id,
        max_doc_id,
    }
}

#[test]
fn test_verify_clean_segments() {
    let tmp = make_temp_dir("verify_clean");
    let index_dir = &tmp;

    let mut manifest = Manifest::new();
    let seg1 = write_test_segment(index_dir, 1, &[1, 2, 3, 4, 5]);
    let seg2 = write_test_segment(index_dir, 2, &[6, 7, 8, 9, 10]);

    manifest.segments.push(seg1);
    manifest.segments.push(seg2);
    manifest.save_atomic(index_dir).unwrap();

    let report = verify_index(index_dir).unwrap();
    assert!(report.all_segments_valid);
    assert_eq!(report.total_segments, 2);
    assert_eq!(report.total_docs, 10);
    assert_eq!(report.total_tombstones, 0);
    assert_eq!(report.active_docs, 10);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_verify_detects_data_corruption() {
    let tmp = make_temp_dir("verify_corrupt");
    let index_dir = &tmp;

    let mut manifest = Manifest::new();
    let seg1 = write_test_segment(index_dir, 1, &[1, 2, 3]);
    manifest.segments.push(seg1.clone());
    manifest.save_atomic(index_dir).unwrap();

    // Corrupt one byte in the data file
    let data_path = index_dir.join(&seg1.data_file);
    let mut bytes = std::fs::read(&data_path).unwrap();
    bytes[40] ^= 0xff; // flip bits in payload
    std::fs::write(&data_path, &bytes).unwrap();

    let report = verify_index(index_dir).unwrap();
    assert!(!report.all_segments_valid);
    assert!(!report.segments[0].data_file_valid);
    assert!(!report.segments[0].errors.is_empty());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn test_tombstone_and_compaction_flow() {
    let tmp = make_temp_dir("compaction_flow");
    let index_dir = &tmp;

    let mut manifest = Manifest::new();
    let seg1 = write_test_segment(index_dir, 1, &[10, 11, 12, 13]);
    let seg2 = write_test_segment(index_dir, 2, &[20, 21, 22]);

    manifest.segments.push(seg1);
    manifest.segments.push(seg2);
    manifest.save_atomic(index_dir).unwrap();

    // Mark doc 11 and 21 as tombstoned
    assert!(mark_tombstone(index_dir, 11).unwrap());
    assert!(mark_tombstone(index_dir, 21).unwrap());
    // Marking already tombstoned should return false
    assert!(!mark_tombstone(index_dir, 11).unwrap());

    // Verify reports 2 tombstones
    let rep_before = verify_index(index_dir).unwrap();
    assert!(rep_before.all_segments_valid);
    assert_eq!(rep_before.total_docs, 7);
    assert_eq!(rep_before.total_tombstones, 2);
    assert_eq!(rep_before.active_docs, 5);

    // Compact all segments
    let comp_res = compact_segments(index_dir, None).unwrap();
    assert_eq!(comp_res.old_segments_count, 2);
    assert_eq!(comp_res.docs_retained, 5);
    assert_eq!(comp_res.docs_purged, 2);
    assert!(comp_res.merged_segment.is_some());

    // Verify after compaction
    let rep_after = verify_index(index_dir).unwrap();
    assert!(rep_after.all_segments_valid);
    assert_eq!(rep_after.total_segments, 1);
    assert_eq!(rep_after.total_docs, 5);
    assert_eq!(rep_after.total_tombstones, 0);
    assert_eq!(rep_after.active_docs, 5);

    // Old segment files must be gone
    assert!(!index_dir.join("seg_00000001.data").exists());
    assert!(!index_dir.join("seg_00000001.idx").exists());
    assert!(!index_dir.join("seg_00000002.data").exists());
    assert!(!index_dir.join("seg_00000002.idx").exists());

    // New segment files must exist
    let new_seg_id = comp_res.merged_segment.unwrap().segment_id;
    assert!(index_dir
        .join(format!("seg_{:08}.data", new_seg_id))
        .exists());
    assert!(index_dir
        .join(format!("seg_{:08}.idx", new_seg_id))
        .exists());

    let _ = std::fs::remove_dir_all(&tmp);
}
