use crate::error::Result;
use crate::generator::{generate_synthetic_dataset, SimpleRng};
use imgseek_index::{
    analyze_index_segment, DataSegmentWriter, IndexSegmentReader, IndexSegmentWriter,
    MihEnsembleIndex, OccupancyReport,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleBenchmarkResult {
    pub doc_count: usize,
    pub index_duration_ms: u64,
    pub throughput_imgs_per_sec: f64,
    pub data_bytes: u64,
    pub idx_bytes: u64,
    pub bytes_per_image: f64,
    pub query_count: usize,
    pub p50_latency_us: u64,
    pub p90_latency_us: u64,
    pub p95_latency_us: u64,
    pub p99_latency_us: u64,
    pub max_latency_us: u64,
    pub mean_latency_us: f64,
    pub occupancy: OccupancyReport,
}

pub fn run_scale_benchmark<P: AsRef<Path>>(
    dataset_sizes: &[usize],
    num_queries: usize,
    work_dir: P,
) -> Result<Vec<ScaleBenchmarkResult>> {
    let base_dir = work_dir.as_ref();
    let mut results = Vec::new();

    for &count in dataset_sizes {
        let seg_dir = base_dir.join(format!("scale_{}", count));
        fs::create_dir_all(&seg_dir)?;

        let records = generate_synthetic_dataset(count, 0.35, 12345 + count as u64);

        // 1. Indexing Benchmark
        let t_index_start = Instant::now();
        let mut data_writer = DataSegmentWriter::create(&seg_dir, 1)?;
        let mut ensemble = MihEnsembleIndex::new();

        for rec in &records {
            data_writer.add_record(rec)?;
            ensemble.insert_record(rec);
        }

        let (_, _data_crc, _min_id, _max_id) = data_writer.finish()?;
        let (_, _idx_crc) = IndexSegmentWriter::write_index(&seg_dir, 1, &ensemble)?;

        let index_duration = t_index_start.elapsed();
        let index_duration_ms = index_duration.as_millis() as u64;
        let throughput_imgs_per_sec = (count as f64) / index_duration.as_secs_f64();

        // 2. Footprint Measurement
        let data_bytes = fs::metadata(seg_dir.join("seg_00000001.data"))?.len();
        let idx_bytes = fs::metadata(seg_dir.join("seg_00000001.idx"))?.len();
        let total_bytes = data_bytes + idx_bytes;
        let bytes_per_image = (total_bytes as f64) / (count as f64);

        // 3. Bucket Occupancy Analysis
        let idx_path = seg_dir.join("seg_00000001.idx");
        let idx_reader = IndexSegmentReader::open(&idx_path)?;
        let occupancy = analyze_index_segment(&idx_reader)?;

        // 4. Query Latency Benchmark
        let mut query_rng = SimpleRng::new(9999 + count as u64);
        let mut latencies_us = Vec::with_capacity(num_queries);

        for i in 0..num_queries {
            let mode = i % 4;
            let (query_phash, tau) = match mode {
                0 => {
                    // Exact match
                    let target = &records[(query_rng.next_u32() as usize) % records.len()];
                    (target.phash, 0u8)
                }
                1 => {
                    // Near duplicate: flip 2 bits, tau = 8
                    let target = &records[(query_rng.next_u32() as usize) % records.len()];
                    let bit1 = (query_rng.next_u32() % 64) as usize;
                    let bit2 = (query_rng.next_u32() % 64) as usize;
                    let p = target.phash ^ (1u64 << bit1) ^ (1u64 << bit2);
                    (p, 8u8)
                }
                2 => {
                    // Boundary near duplicate: flip 4 bits, tau = 12
                    let target = &records[(query_rng.next_u32() as usize) % records.len()];
                    let bit1 = (query_rng.next_u32() % 64) as usize;
                    let bit2 = (query_rng.next_u32() % 64) as usize;
                    let bit3 = (query_rng.next_u32() % 64) as usize;
                    let bit4 = (query_rng.next_u32() % 64) as usize;
                    let p = target.phash
                        ^ (1u64 << bit1)
                        ^ (1u64 << bit2)
                        ^ (1u64 << bit3)
                        ^ (1u64 << bit4);
                    (p, 12u8)
                }
                _ => {
                    // Random noise query, tau = 12
                    (query_rng.next_u64(), 12u8)
                }
            };

            let t_query_start = Instant::now();
            let _candidates = idx_reader.search_phash_candidates(query_phash, tau)?;
            let elapsed_us = t_query_start.elapsed().as_micros() as u64;
            latencies_us.push(elapsed_us);
        }

        latencies_us.sort_unstable();

        let p50 = latencies_us[(latencies_us.len() * 50) / 100];
        let p90 = latencies_us[(latencies_us.len() * 90) / 100];
        let p95 = latencies_us[(latencies_us.len() * 95) / 100];
        let p99 = latencies_us[(latencies_us.len() * 99) / 100];
        let max = latencies_us[latencies_us.len() - 1];
        let sum: u64 = latencies_us.iter().sum();
        let mean = (sum as f64) / (latencies_us.len() as f64);

        results.push(ScaleBenchmarkResult {
            doc_count: count,
            index_duration_ms,
            throughput_imgs_per_sec,
            data_bytes,
            idx_bytes,
            bytes_per_image,
            query_count: num_queries,
            p50_latency_us: p50,
            p90_latency_us: p90,
            p95_latency_us: p95,
            p99_latency_us: p99,
            max_latency_us: max,
            mean_latency_us: mean,
            occupancy,
        });

        // Clean up temporary segment files
        let _ = fs::remove_dir_all(&seg_dir);
    }

    Ok(results)
}
