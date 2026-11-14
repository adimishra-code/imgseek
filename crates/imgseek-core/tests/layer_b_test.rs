use imgseek_core::config::NormalizeConfig;
use imgseek_core::{ahash_from_luma, dhash_from_luma, normalize, phash_from_luma, whash_from_luma};
use serde_json::Value;
use std::fs;
use std::path::Path;

#[test]
fn test_layer_b_full_pipeline_tolerance() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_json = manifest_dir.join("../../tests/fixtures/golden_hashes.json");
    let synthetic_dir = manifest_dir.join("../../tests/fixtures/synthetic");

    let content = fs::read_to_string(&fixture_json).expect("read golden_hashes.json");
    let records: Vec<Value> = serde_json::from_str(&content).expect("parse json");

    let cfg = NormalizeConfig::default();

    let mut dists_ahash = Vec::new();
    let mut dists_dhash = Vec::new();
    let mut dists_phash = Vec::new();
    let mut dists_whash = Vec::new();

    for r in &records {
        let filename = r["filename"].as_str().expect("filename");
        let file_path = synthetic_dir.join(filename);
        let bytes = fs::read(&file_path).unwrap_or_else(|_| panic!("read {:?}", file_path));

        let norm =
            normalize(&bytes, &cfg).unwrap_or_else(|e| panic!("normalize {}: {:?}", filename, e));

        let a = ahash_from_luma(norm.width, norm.height, &norm.luma);
        let d = dhash_from_luma(norm.width, norm.height, &norm.luma);
        let p = phash_from_luma(norm.width, norm.height, &norm.luma);
        let w = whash_from_luma(norm.width, norm.height, &norm.luma);

        let exp_a = r["layer_b"]["ahash"].as_u64().expect("exp ahash");
        let exp_d = r["layer_b"]["dhash"].as_u64().expect("exp dhash");
        let exp_p = r["layer_b"]["phash"].as_u64().expect("exp phash");
        let exp_w = r["layer_b"]["whash"].as_u64().expect("exp whash");

        dists_ahash.push((a ^ exp_a).count_ones());
        dists_dhash.push((d ^ exp_d).count_ones());
        dists_phash.push((p ^ exp_p).count_ones());
        dists_whash.push((w ^ exp_w).count_ones());
    }

    let n = records.len() as f64;
    let mean_a = dists_ahash.iter().sum::<u32>() as f64 / n;
    let max_a = *dists_ahash.iter().max().unwrap();

    let mean_d = dists_dhash.iter().sum::<u32>() as f64 / n;
    let max_d = *dists_dhash.iter().max().unwrap();

    let mean_p = dists_phash.iter().sum::<u32>() as f64 / n;
    let max_p = *dists_phash.iter().max().unwrap();

    let mean_w = dists_whash.iter().sum::<u32>() as f64 / n;
    let max_w = *dists_whash.iter().max().unwrap();

    println!("Layer B Statistics across {} images:", records.len());
    println!("  aHash: mean = {:.4}, max = {}", mean_a, max_a);
    println!("  dHash: mean = {:.4}, max = {}", mean_d, max_d);
    println!("  pHash: mean = {:.4}, max = {}", mean_p, max_p);
    println!("  wHash: mean = {:.4}, max = {}", mean_w, max_w);

    assert!(mean_a <= 2.0, "aHash mean distance {mean_a} > 2.0");
    assert!(max_a <= 6, "aHash max distance {max_a} > 6");

    assert!(mean_d <= 2.0, "dHash mean distance {mean_d} > 2.0");
    assert!(max_d <= 6, "dHash max distance {max_d} > 6");

    assert!(mean_p <= 2.0, "pHash mean distance {mean_p} > 2.0");
    assert!(max_p <= 6, "pHash max distance {max_p} > 6");

    assert!(mean_w <= 2.0, "wHash mean distance {mean_w} > 2.0");
    assert!(max_w <= 6, "wHash max distance {max_w} > 6");
}
