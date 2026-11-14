use crate::error::Result;
use crate::mih::HashAlgoId;
use crate::segment::IndexSegmentReader;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkTableOccupancy {
    pub algo: String,
    pub chunk_index: usize,
    pub total_postings: usize,
    pub non_empty_buckets: usize,
    pub empty_buckets: usize,
    pub occupancy_ratio: f64,
    pub min_bucket_len: usize,
    pub max_bucket_len: usize,
    pub mean_bucket_len: f64,
    pub p50_bucket_len: usize,
    pub p90_bucket_len: usize,
    pub p99_bucket_len: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OccupancyReport {
    pub doc_count: u32,
    pub total_tables: usize,
    pub avg_occupancy_ratio: f64,
    pub max_collision_bucket: usize,
    pub tables: Vec<ChunkTableOccupancy>,
}

pub fn analyze_index_segment(reader: &IndexSegmentReader) -> Result<OccupancyReport> {
    let mut table_reports = Vec::new();

    let algos = [
        (HashAlgoId::PHash, 4),
        (HashAlgoId::WHash, 4),
        (HashAlgoId::DHash, 4),
        (HashAlgoId::Pdq, 16),
    ];

    for (algo, num_chunks) in algos {
        for chunk_idx in 0..num_chunks {
            let view = reader.chunk_view(algo, chunk_idx)?;
            let mut lengths: Vec<usize> = Vec::with_capacity(65536);
            let mut total_postings = 0usize;
            let mut non_empty_buckets = 0usize;

            for key in 0..65536u32 {
                let k = key as u16;
                if !view.is_empty(k) {
                    let docs = view.get(k)?;
                    let len = docs.len();
                    if len > 0 {
                        lengths.push(len);
                        total_postings += len;
                        non_empty_buckets += 1;
                    }
                }
            }

            lengths.sort_unstable();

            let empty_buckets = 65536 - non_empty_buckets;
            let occupancy_ratio = (non_empty_buckets as f64) / 65536.0;

            let (min_bucket_len, max_bucket_len, mean_bucket_len, p50, p90, p99) =
                if lengths.is_empty() {
                    (0, 0, 0.0, 0, 0, 0)
                } else {
                    let min = lengths[0];
                    let max = lengths[lengths.len() - 1];
                    let mean = (total_postings as f64) / (non_empty_buckets as f64);
                    let p50 = lengths[(lengths.len() * 50) / 100];
                    let p90 = lengths[(lengths.len() * 90) / 100];
                    let p99 = lengths[(lengths.len() * 99) / 100];
                    (min, max, mean, p50, p90, p99)
                };

            let algo_name = match algo {
                HashAlgoId::PHash => "phash",
                HashAlgoId::WHash => "whash",
                HashAlgoId::DHash => "dhash",
                HashAlgoId::AHash => "ahash",
                HashAlgoId::Pdq => "pdq",
            }
            .to_string();

            table_reports.push(ChunkTableOccupancy {
                algo: algo_name,
                chunk_index: chunk_idx,
                total_postings,
                non_empty_buckets,
                empty_buckets,
                occupancy_ratio,
                min_bucket_len,
                max_bucket_len,
                mean_bucket_len,
                p50_bucket_len: p50,
                p90_bucket_len: p90,
                p99_bucket_len: p99,
            });
        }
    }

    let total_tables = table_reports.len();
    let sum_occupancy: f64 = table_reports.iter().map(|t| t.occupancy_ratio).sum();
    let avg_occupancy_ratio = if total_tables > 0 {
        sum_occupancy / (total_tables as f64)
    } else {
        0.0
    };
    let max_collision_bucket = table_reports
        .iter()
        .map(|t| t.max_bucket_len)
        .max()
        .unwrap_or(0);

    Ok(OccupancyReport {
        doc_count: reader.doc_count(),
        total_tables,
        avg_occupancy_ratio,
        max_collision_bucket,
        tables: table_reports,
    })
}
