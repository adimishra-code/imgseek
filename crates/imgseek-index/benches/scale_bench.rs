use imgseek_index::{
    analyze_index_segment, DataSegmentWriter, DocumentRecord, IndexSegmentReader,
    IndexSegmentWriter, MihEnsembleIndex,
};
use std::fs;
use std::time::Instant;

fn main() {
    let tmp = std::env::temp_dir().join(format!("imgseek_cargo_bench_{}", std::process::id()));
    fs::create_dir_all(&tmp).expect("create bench dir");

    println!("Running MIH Scale Benchmark (10,000 records)...");
    let count = 10_000;
    let mut records = Vec::with_capacity(count);

    let mut state: u64 = 0x1234_5678_9abc_def0;
    for i in 0..count {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let p = state;
        let mut pdq = [0u8; 32];
        pdq[0..8].copy_from_slice(&state.to_le_bytes());

        records.push(DocumentRecord {
            doc_id: (i + 1) as u32,
            phash: p,
            whash: p ^ 0xffff,
            dhash: p ^ 0xaaaa,
            ahash: p ^ 0x5555,
            pdq,
            pdq_quality: 90,
            width: 800,
            height: 600,
            chroma_ref: [0x33; 32],
        });
    }

    let t0 = Instant::now();
    let mut data_writer = DataSegmentWriter::create(&tmp, 1).unwrap();
    let mut ensemble = MihEnsembleIndex::new();

    for r in &records {
        data_writer.add_record(r).unwrap();
        ensemble.insert_record(r);
    }

    data_writer.finish().unwrap();
    IndexSegmentWriter::write_index(&tmp, 1, &ensemble).unwrap();
    let index_ms = t0.elapsed().as_millis();
    println!("Indexing completed in {} ms", index_ms);

    let idx_reader = IndexSegmentReader::open(tmp.join("seg_00000001.idx")).unwrap();
    let occ = analyze_index_segment(&idx_reader).unwrap();
    println!(
        "Occupancy: avg={:.1}%, max_bucket={}",
        occ.avg_occupancy_ratio * 100.0,
        occ.max_collision_bucket
    );

    let query_count = 1000;
    let mut latencies_us = Vec::with_capacity(query_count);

    for i in 0..query_count {
        let q = records[i % records.len()].phash ^ ((i as u64) & 0xf);
        let start = Instant::now();
        let _hits = idx_reader.search_phash_candidates(q, 8).unwrap();
        latencies_us.push(start.elapsed().as_micros() as u64);
    }

    latencies_us.sort_unstable();
    let p50 = latencies_us[(latencies_us.len() * 50) / 100];
    let p99 = latencies_us[(latencies_us.len() * 99) / 100];
    println!("Query Latency: p50 = {} us, p99 = {} us", p50, p99);
    assert!(p99 < 500_000, "p99 must be < 500ms");

    let _ = fs::remove_dir_all(&tmp);
    println!("Benchmark completed successfully!");
}
