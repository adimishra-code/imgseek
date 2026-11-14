use imgseek_index::{
    HashAlgoId, IndexSegmentReader, IndexSegmentWriter, MihEnsembleIndex, MihFamilyIndex,
};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Deterministic 64-bit pseudo-random number generator (PCG / Xorshift64).
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 {
                0xCAFE_BABE_DEAD_BEEF
            } else {
                seed
            },
        }
    }

    fn next_u64(&mut self) -> u64 {
        // 64-bit Xorshift*
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_bounded(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

/// Compute Hamming distance between two 64-bit values.
fn hamming_distance_64(a: u64, b: u64) -> u8 {
    (a ^ b).count_ones() as u8
}

/// Compute Hamming distance between two 256-bit hashes.
fn hamming_distance_256(a: &[u8; 32], b: &[u8; 32]) -> u16 {
    let mut dist = 0u16;
    for i in 0..32 {
        dist += (a[i] ^ b[i]).count_ones() as u16;
    }
    dist
}

#[test]
fn test_differential_mih_vs_brute_force_random_distribution() {
    let mut rng = DeterministicRng::new(0x1234_5678_9ABC_DEF0);

    // 1. Generate 1,500 random 64-bit hashes
    let n = 1500;
    let mut ground_truth: Vec<(u32, u64)> = Vec::with_capacity(n);
    let mut mih = MihFamilyIndex::new_64(HashAlgoId::PHash);

    for doc_id in 1..=(n as u32) {
        let hash = rng.next_u64();
        ground_truth.push((doc_id, hash));
        mih.insert_64(hash, doc_id);
    }

    // 2. Test queries: existing hashes, perturbed queries, and random queries
    let mut query_hashes = Vec::new();

    // Query with some indexed hashes (should have exact match)
    for &idx in &[0, 10, 100, 500, 1000] {
        query_hashes.push(ground_truth[idx].1);
    }

    // Query with 1-bit, 2-bit, 4-bit, 8-bit, 12-bit perturbations of indexed hashes
    for &(idx, bits) in &[(5, 1), (25, 2), (50, 4), (75, 8), (120, 12)] {
        let mut perturbed = ground_truth[idx].1;
        for b in 0..bits {
            perturbed ^= 1u64 << ((b * 5) % 64);
        }
        query_hashes.push(perturbed);
    }

    // Query with completely random hashes
    for _ in 0..5 {
        query_hashes.push(rng.next_u64());
    }

    // 3. For every query and tau in 0..=12, verify 100% recall
    for &query in &query_hashes {
        for tau in 0..=12 {
            // Brute-force ground truth
            let expected_matches: HashSet<u32> = ground_truth
                .iter()
                .filter(|(_, h)| hamming_distance_64(query, *h) <= tau)
                .map(|(id, _)| *id)
                .collect();

            // MIH candidate retrieval
            let candidates = mih.search_neighbors_64(query, tau);
            let candidate_set: HashSet<u32> = candidates.iter().map(|c| c.doc_id).collect();

            // Recall check: All expected matches MUST be in candidate set (Zero false negatives)
            for &expected_id in &expected_matches {
                assert!(
                    candidate_set.contains(&expected_id),
                    "Recall failure: doc {} missing from MIH candidates for tau={}",
                    expected_id,
                    tau
                );
            }

            // Exact verification check: filtering candidates by full distance produces exact set
            let verified: HashSet<u32> = candidates
                .iter()
                .filter(|c| {
                    let doc_hash = ground_truth[(c.doc_id - 1) as usize].1;
                    hamming_distance_64(query, doc_hash) <= tau
                })
                .map(|c| c.doc_id)
                .collect();

            assert_eq!(
                verified, expected_matches,
                "Verified set mismatch for query 0x{:016X} at tau={}",
                query, tau
            );
        }
    }
}

#[test]
fn test_differential_mih_vs_brute_force_clustered_distribution() {
    let mut rng = DeterministicRng::new(0xDEAD_BEEF_CAFE_BABE);

    // Generate clustered distribution: 15 cluster centers, each with 80 perturbations
    let num_clusters = 15;
    let items_per_cluster = 80;
    let mut ground_truth: Vec<(u32, u64)> = Vec::new();
    let mut mih = MihFamilyIndex::new_64(HashAlgoId::DHash);

    let mut doc_id = 1;
    for _ in 0..num_clusters {
        let center = rng.next_u64();
        for _ in 0..items_per_cluster {
            let mut h = center;
            let num_flips = (rng.next_bounded(8) + 1) as usize;
            for _ in 0..num_flips {
                let bit = rng.next_bounded(64);
                h ^= 1u64 << bit;
            }
            ground_truth.push((doc_id, h));
            mih.insert_64(h, doc_id);
            doc_id += 1;
        }
    }

    // Test queries centered around clusters and in between
    for center_idx in 0..num_clusters {
        let query = ground_truth[center_idx * items_per_cluster].1;
        for tau in [0, 1, 2, 4, 6, 8, 10, 12] {
            let expected_matches: HashSet<u32> = ground_truth
                .iter()
                .filter(|(_, h)| hamming_distance_64(query, *h) <= tau)
                .map(|(id, _)| *id)
                .collect();

            let candidates = mih.search_neighbors_64(query, tau);
            let candidate_set: HashSet<u32> = candidates.iter().map(|c| c.doc_id).collect();

            for &expected_id in &expected_matches {
                assert!(
                    candidate_set.contains(&expected_id),
                    "Clustered recall failure: doc {} missing for tau={}",
                    expected_id,
                    tau
                );
            }

            let verified: HashSet<u32> = candidates
                .iter()
                .filter(|c| {
                    let doc_hash = ground_truth[(c.doc_id - 1) as usize].1;
                    hamming_distance_64(query, doc_hash) <= tau
                })
                .map(|c| c.doc_id)
                .collect();

            assert_eq!(verified, expected_matches);
        }
    }
}

#[test]
fn test_differential_mih_vs_brute_force_real_fixtures() {
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests/fixtures/golden_hashes.json");

    if !golden_path.exists() {
        return;
    }

    let raw_json = fs::read_to_string(&golden_path).expect("read golden json");
    let records: serde_json::Value = serde_json::from_str(&raw_json).expect("parse json");
    let entries = records.as_array().expect("array of records");

    let mut ground_truth_phash: Vec<(u32, u64)> = Vec::new();
    let mut ensemble = MihEnsembleIndex::new();

    let mut ground_truth_pdq: Vec<(u32, [u8; 32])> = Vec::new();

    for (idx, entry) in entries.iter().enumerate() {
        let doc_id = (idx + 1) as u32;
        let phash = entry["layer_b"]["phash"].as_u64().unwrap();
        let whash = entry["layer_b"]["whash"].as_u64().unwrap();
        let dhash = entry["layer_b"]["dhash"].as_u64().unwrap();

        let pdq_hex = entry["layer_a"]["pdq"].as_str().unwrap();
        let mut pdq = [0u8; 32];
        for i in 0..32 {
            pdq[i] = u8::from_str_radix(&pdq_hex[2 * i..2 * i + 2], 16).unwrap();
        }
        let pdq_quality = entry["layer_a"]["pdq_quality"].as_u64().unwrap() as u16;

        ground_truth_phash.push((doc_id, phash));
        ground_truth_pdq.push((doc_id, pdq));

        let doc_rec = imgseek_index::DocumentRecord {
            doc_id,
            phash,
            whash,
            dhash,
            ahash: 0,
            pdq,
            pdq_quality,
            width: 512,
            height: 512,
            chroma_ref: [0; 32],
        };
        ensemble.insert_record(&doc_rec);
    }

    // Write index to a segment file on disk and verify with IndexSegmentReader
    let test_dir = std::env::temp_dir().join(format!(
        "real_diff_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let (seg_path, _) =
        IndexSegmentWriter::write_index(&test_dir, 10, &ensemble).expect("write segment");
    let reader = IndexSegmentReader::open(&seg_path).expect("open segment");

    // Test queries across golden images with varying tau
    for &(query_idx, tau) in &[
        (0, 0),
        (5, 2),
        (10, 4),
        (20, 6),
        (35, 8),
        (50, 10),
        (80, 12),
    ] {
        let query_hash = ground_truth_phash[query_idx].1;

        let expected_matches: HashSet<u32> = ground_truth_phash
            .iter()
            .filter(|(_, h)| hamming_distance_64(query_hash, *h) <= tau)
            .map(|(id, _)| *id)
            .collect();

        // Search via reader
        let candidates = reader
            .search_phash_candidates(query_hash, tau)
            .expect("search candidates");
        let candidate_set: HashSet<u32> = candidates.iter().map(|c| c.doc_id).collect();

        for &expected_id in &expected_matches {
            assert!(
                candidate_set.contains(&expected_id),
                "Real corpus recall failure: doc {} missing at tau={}",
                expected_id,
                tau
            );
        }

        let verified: HashSet<u32> = candidates
            .iter()
            .filter(|c| {
                let doc_hash = ground_truth_phash[(c.doc_id - 1) as usize].1;
                hamming_distance_64(query_hash, doc_hash) <= tau
            })
            .map(|c| c.doc_id)
            .collect();

        assert_eq!(verified, expected_matches);
    }

    // Also test PDQ across golden images with tau in [0, 16]
    for &(query_idx, tau) in &[(0, 0), (10, 16), (25, 16)] {
        let query_pdq = ground_truth_pdq[query_idx].1;

        let expected_matches: HashSet<u32> = ground_truth_pdq
            .iter()
            .filter(|(_, h)| hamming_distance_256(&query_pdq, h) <= tau as u16)
            .map(|(id, _)| *id)
            .collect();

        let candidates = reader
            .search_pdq_candidates(&query_pdq, tau)
            .expect("search pdq candidates");
        let candidate_set: HashSet<u32> = candidates.iter().map(|c| c.doc_id).collect();

        for &expected_id in &expected_matches {
            assert!(
                candidate_set.contains(&expected_id),
                "Real PDQ recall failure: doc {} missing at tau={}",
                expected_id,
                tau
            );
        }

        let verified: HashSet<u32> = candidates
            .iter()
            .filter(|c| {
                let doc_hash = &ground_truth_pdq[(c.doc_id - 1) as usize].1;
                hamming_distance_256(&query_pdq, doc_hash) <= tau as u16
            })
            .map(|c| c.doc_id)
            .collect();

        assert_eq!(verified, expected_matches);
    }

    let _ = fs::remove_dir_all(&test_dir);
}
