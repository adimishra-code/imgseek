use clap::Parser;
use imgseek_eval::run_scale_benchmark;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "imgseek-bench",
    about = "imgseek Scale Benchmark & Occupancy Reporter"
)]
struct Args {
    /// Comma-separated list of dataset sizes to benchmark
    #[arg(short, long, default_value = "10000,100000")]
    sizes: String,

    /// Number of search queries to probe per scale
    #[arg(short, long, default_value = "1000")]
    queries: usize,

    /// Temporary working directory for benchmark segments
    #[arg(short, long)]
    work_dir: Option<PathBuf>,

    /// Emit report in JSON format
    #[arg(long)]
    json: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let sizes: Vec<usize> = args
        .sizes
        .split(',')
        .map(|s| s.trim().parse::<usize>())
        .collect::<Result<_, _>>()?;

    let work_dir = args.work_dir.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("imgseek_scale_bench_{}", std::process::id()))
    });
    std::fs::create_dir_all(&work_dir)?;

    println!("Starting imgseek scale benchmark for sizes: {:?}", sizes);
    println!("Query sample count per scale: {}", args.queries);

    let results = run_scale_benchmark(&sizes, args.queries, &work_dir)?;

    let _ = std::fs::remove_dir_all(&work_dir);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!("\n=================== SCALE BENCHMARK REPORT ===================");
        for res in &results {
            println!("\n--- Dataset Size: {} images ---", res.doc_count);
            println!(
                "Indexing Time:       {} ms ({:.0} images/sec)",
                res.index_duration_ms, res.throughput_imgs_per_sec
            );
            println!(
                "On-Disk Segment:     .data = {} KB, .idx = {} KB (Total = {:.1} B/image)",
                res.data_bytes / 1024,
                res.idx_bytes / 1024,
                res.bytes_per_image
            );
            println!(
                "Query Latency (us):  p50 = {} us, p90 = {} us, p95 = {} us, p99 = {} us, max = {} us (mean = {:.1} us)",
                res.p50_latency_us,
                res.p90_latency_us,
                res.p95_latency_us,
                res.p99_latency_us,
                res.max_latency_us,
                res.mean_latency_us
            );
            println!(
                "P99 Under 500ms:     {} (p99 = {:.3} ms)",
                if res.p99_latency_us < 500_000 {
                    "PASS (TARGET MET)"
                } else {
                    "FAIL"
                },
                (res.p99_latency_us as f64) / 1000.0
            );

            println!("MIH Occupancy Summary:");
            println!("  Total chunk tables:     {}", res.occupancy.total_tables);
            println!(
                "  Avg non-empty buckets:  {:.1}%",
                res.occupancy.avg_occupancy_ratio * 100.0
            );
            println!(
                "  Max bucket collision:   {} postings",
                res.occupancy.max_collision_bucket
            );

            println!("  Selected Tables:");
            for t in res.occupancy.tables.iter().take(4) {
                println!(
                    "    {} chunk {}: non-empty={}/65536 ({:.1}%), max_bucket={}, mean_bucket={:.2}, p50={}, p99={}",
                    t.algo,
                    t.chunk_index,
                    t.non_empty_buckets,
                    t.occupancy_ratio * 100.0,
                    t.max_bucket_len,
                    t.mean_bucket_len,
                    t.p50_bucket_len,
                    t.p99_bucket_len
                );
            }
        }
        println!("\n==============================================================\n");
    }

    Ok(())
}
