use imgseek_eval::run_scale_benchmark;

#[test]
fn test_scale_benchmark_10k_and_latency_target() {
    let tmp = std::env::temp_dir().join(format!(
        "imgseek_bench_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    // Benchmark 10,000 images with 100 queries
    let results = run_scale_benchmark(&[10_000], 100, &tmp).unwrap();
    assert_eq!(results.len(), 1);

    let res = &results[0];
    assert_eq!(res.doc_count, 10_000);
    assert_eq!(res.query_count, 100);

    // Assert TARGET: exact-tier query p99 < 500 ms (500,000 us)
    assert!(
        res.p99_latency_us < 500_000,
        "p99 latency {} us must be < 500,000 us",
        res.p99_latency_us
    );

    // Assert reasonable occupancy metrics
    assert_eq!(res.occupancy.total_tables, 28);
    assert!(res.occupancy.avg_occupancy_ratio > 0.0);
    assert!(res.occupancy.max_collision_bucket > 0);

    let _ = std::fs::remove_dir_all(&tmp);
}
