use std::path::Path;

const FORBIDDEN_NETWORK_CRATES: &[&str] = &[
    "reqwest",
    "hyper",
    "ureq",
    "isahc",
    "surf",
    "attohttpc",
    "curl",
];

#[test]
fn test_query_path_has_zero_network_crates() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lockfile_path = manifest_dir.join("../../Cargo.lock");

    if lockfile_path.exists() {
        let lock_content =
            std::fs::read_to_string(&lockfile_path).expect("Failed to read Cargo.lock");

        // The query engine crates that must remain strictly offline:
        let offline_crates = [
            "name = \"imgseek-core\"",
            "name = \"imgseek-features\"",
            "name = \"imgseek-index\"",
            "name = \"imgseek-store\"",
            "name = \"imgseek-query\"",
        ];

        // Ensure each forbidden crate is never depended on by query path
        for forbidden in FORBIDDEN_NETWORK_CRATES {
            let needle = format!("name = \"{}\"", forbidden);
            if lock_content.contains(&needle) {
                // If the crate exists in workspace (e.g. for crawler in phase 4),
                // verify it is NOT in dependencies of imgseek-query or its core dependencies.
                for query_crate in &offline_crates {
                    if let Some(pos) = lock_content.find(query_crate) {
                        let block = &lock_content[pos..];
                        let block_end = block[1..].find("[[package]]").unwrap_or(block.len());
                        let crate_block = &block[..block_end];
                        assert!(
                            !crate_block.contains(&format!("\"{}\"", forbidden)),
                            "Query crate {} contains forbidden network dependency {}",
                            query_crate,
                            forbidden
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn test_query_engine_offline_determinism() {
    use imgseek_core::Config;
    use imgseek_query::{QueryEngine, QueryOptions};
    use imgseek_store::{BlobStore, Database};
    use std::fs;

    let tmp_dir = std::env::temp_dir().join(format!(
        "imgseek_offline_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&tmp_dir).unwrap();

    let db_path = tmp_dir.join("test.db");
    let blob_dir = tmp_dir.join("blobs");
    let index_dir = tmp_dir.join("index");
    fs::create_dir_all(&blob_dir).unwrap();
    fs::create_dir_all(&index_dir).unwrap();

    let db = Database::open(&db_path).unwrap();
    let blob_store = BlobStore::new(&blob_dir).unwrap();

    let mut config = Config::default();
    config.index.dir = index_dir;

    let engine = QueryEngine::new(&db, &blob_store, &config);

    // Run query with dummy image data - must execute purely in-memory/on-disk without any network I/O
    let options = QueryOptions {
        limit: 10,
        min_score: 0.5,
        corpus: None,
    };

    // Minimal valid 2x2 24-bit uncompressed BMP image (no CRCs or complex chunks)
    let bmp_bytes: [u8; 70] = [
        0x42, 0x4d, 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 1,
        0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0,
        0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0,
    ];

    let results = engine.query_bytes(&bmp_bytes, &options).unwrap();

    // In an empty index, query completes cleanly returning 0 results with zero errors
    assert_eq!(results.len(), 0);

    let _ = fs::remove_dir_all(&tmp_dir);
}
