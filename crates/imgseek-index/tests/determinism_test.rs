use imgseek_index::{
    DataSegmentReader, DataSegmentWriter, DocumentRecord, IndexSegmentReader, IndexSegmentWriter,
    MihEnsembleIndex,
};
use std::fs;
use std::path::PathBuf;

fn make_temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "imgseek_det_{}_{}_{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn build_test_dataset(count: usize) -> Vec<DocumentRecord> {
    let mut records = Vec::with_capacity(count);
    let mut state: u64 = 0xbeef_cafe_1234_5678;

    for i in 0..count {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let phash = state;
        let whash = state.rotate_left(13) ^ 0x0123_4567_89ab_cdef;
        let dhash = state.rotate_right(7) ^ 0xfedc_ba98_7654_3210;
        let ahash = (state ^ 0x5555_5555_5555_5555).wrapping_add(i as u64);

        let mut pdq = [0u8; 32];
        pdq[0..8].copy_from_slice(&state.to_le_bytes());
        pdq[8..16].copy_from_slice(&whash.to_le_bytes());
        pdq[16..24].copy_from_slice(&dhash.to_le_bytes());
        pdq[24..32].copy_from_slice(&ahash.to_le_bytes());

        records.push(DocumentRecord {
            doc_id: (i + 1) as u32,
            phash,
            whash,
            dhash,
            ahash,
            pdq,
            pdq_quality: 95,
            width: 1920,
            height: 1080,
            chroma_ref: [0x77; 32],
        });
    }

    records
}

#[test]
fn test_segment_generation_is_byte_identical() {
    let dir_a = make_temp_dir("run_a");
    let dir_b = make_temp_dir("run_b");

    let dataset = build_test_dataset(500);

    // Run A
    let mut writer_a = DataSegmentWriter::create(&dir_a, 1).unwrap();
    let mut ensemble_a = MihEnsembleIndex::new();
    for rec in &dataset {
        writer_a.add_record(rec).unwrap();
        ensemble_a.insert_record(rec);
    }
    let (_, data_crc_a, min_id_a, max_id_a) = writer_a.finish().unwrap();
    let (_, idx_crc_a) = IndexSegmentWriter::write_index(&dir_a, 1, &ensemble_a).unwrap();

    // Run B (identical inputs, independent run)
    let mut writer_b = DataSegmentWriter::create(&dir_b, 1).unwrap();
    let mut ensemble_b = MihEnsembleIndex::new();
    for rec in &dataset {
        writer_b.add_record(rec).unwrap();
        ensemble_b.insert_record(rec);
    }
    let (_, data_crc_b, min_id_b, max_id_b) = writer_b.finish().unwrap();
    let (_, idx_crc_b) = IndexSegmentWriter::write_index(&dir_b, 1, &ensemble_b).unwrap();

    // 1. Checksums and metadata must be identical
    assert_eq!(data_crc_a, data_crc_b, "data segment CRC32C must match");
    assert_eq!(idx_crc_a, idx_crc_b, "index segment CRC32C must match");
    assert_eq!(min_id_a, min_id_b);
    assert_eq!(max_id_a, max_id_b);

    // 2. Read full byte payloads and assert byte-identical on disk
    let data_bytes_a = fs::read(dir_a.join("seg_00000001.data")).unwrap();
    let data_bytes_b = fs::read(dir_b.join("seg_00000001.data")).unwrap();
    assert_eq!(
        data_bytes_a, data_bytes_b,
        "Data segments must be byte-identical"
    );

    let idx_bytes_a = fs::read(dir_a.join("seg_00000001.idx")).unwrap();
    let idx_bytes_b = fs::read(dir_b.join("seg_00000001.idx")).unwrap();
    assert_eq!(
        idx_bytes_a, idx_bytes_b,
        "Index segments must be byte-identical"
    );

    // 3. Readers must produce identical iteration and query candidate hits
    let reader_data_a = DataSegmentReader::open(dir_a.join("seg_00000001.data")).unwrap();
    let reader_data_b = DataSegmentReader::open(dir_b.join("seg_00000001.data")).unwrap();
    assert_eq!(reader_data_a.len(), reader_data_b.len());
    for i in 0..reader_data_a.len() {
        assert_eq!(reader_data_a.get(i), reader_data_b.get(i));
    }

    let reader_idx_a = IndexSegmentReader::open(dir_a.join("seg_00000001.idx")).unwrap();
    let reader_idx_b = IndexSegmentReader::open(dir_b.join("seg_00000001.idx")).unwrap();

    let query_hash = dataset[42].phash ^ 0x03; // 2 bit flips
    let hits_a = reader_idx_a.search_phash_candidates(query_hash, 8).unwrap();
    let hits_b = reader_idx_b.search_phash_candidates(query_hash, 8).unwrap();
    assert_eq!(
        hits_a, hits_b,
        "Query candidate hits must be deterministic and identical"
    );

    let _ = fs::remove_dir_all(&dir_a);
    let _ = fs::remove_dir_all(&dir_b);
}
