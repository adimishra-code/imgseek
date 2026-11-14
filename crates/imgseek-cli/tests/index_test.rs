use image::{ImageBuffer, Rgb};
use imgseek_core::Config;
use imgseek_store::{BlobStore, Database};

#[test]
fn test_index_directory_end_to_end() {
    let test_id = format!(
        "imgseek_idx_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_root = std::env::temp_dir().join(test_id);
    let images_dir = temp_root.join("images");
    let sub_dir = images_dir.join("subdir");
    let index_dir = temp_root.join("index");
    let blob_dir = temp_root.join("blobs");

    std::fs::create_dir_all(&sub_dir).expect("create images subdir");
    std::fs::create_dir_all(&index_dir).expect("create index dir");
    std::fs::create_dir_all(&blob_dir).expect("create blob dir");

    // Create valid 100x100 red image
    let img1: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(100, 100, Rgb([255, 0, 0]));
    img1.save(images_dir.join("red.png")).expect("save red.png");

    // Create valid 120x80 blue image in subdirectory
    let img2: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_pixel(120, 80, Rgb([0, 0, 255]));
    img2.save(sub_dir.join("blue.png")).expect("save blue.png");

    // Create non-image text file
    std::fs::write(images_dir.join("readme.txt"), b"Not an image file").expect("save readme.txt");

    // Create hidden image (starts with dot)
    let img_hidden: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(100, 100, Rgb([0, 255, 0]));
    img_hidden
        .save(images_dir.join(".hidden.png"))
        .expect("save .hidden.png");

    // Create tiny image (< min_dimension 64)
    let img_tiny: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(32, 32, Rgb([128, 128, 128]));
    img_tiny
        .save(images_dir.join("tiny.png"))
        .expect("save tiny.png");

    let mut config = Config::default();
    config.index.dir = index_dir.clone();
    config.index.blob_dir = blob_dir.clone();

    // Call CLI index via binary / command or library run_index
    let bin_path = env!("CARGO_BIN_EXE_imgseek");
    let _status = std::process::Command::new(bin_path)
        .arg("index")
        .arg(&images_dir)
        .arg("--corpus")
        .arg("local:test_photos")
        .env("IMGSEEK_INDEX_DIR", &index_dir)
        .output();

    // Verify directly with the database
    let db_path = index_dir.join("metadata.db");
    let db = Database::open(&db_path).expect("open db");
    let blob_store = BlobStore::new(&blob_dir).expect("open blob store");

    // Also run index directly with custom config to verify logic
    let index_args = imgseek_cli_index_args(&images_dir);
    imgseek_run_index_direct(&index_args, &config).expect("run index");

    // Verify docs table
    let docs = db.get_all_docs().expect("get docs");
    assert_eq!(
        docs.len(),
        2,
        "Expected exactly 2 indexed images (red.png and blue.png)"
    );

    for doc in &docs {
        assert!(doc.ahash != 0 || doc.dhash != 0 || doc.phash != 0 || doc.whash != 0);
        assert!(!doc.chroma_blob_ref.is_empty());
        assert!(blob_store.has(&doc.chroma_blob_ref));

        let chroma_bytes = blob_store
            .get(&doc.chroma_blob_ref)
            .expect("read chroma blob");
        assert_eq!(chroma_bytes.len(), 256 * 4);
    }

    // Verify sightings table
    let s1 = db
        .get_sighting(
            "local:test_photos",
            &images_dir.join("red.png").to_string_lossy(),
        )
        .expect("query red sighting");
    assert!(s1.is_some(), "red.png sighting should exist");

    let s2 = db
        .get_sighting(
            "local:test_photos",
            &sub_dir.join("blue.png").to_string_lossy(),
        )
        .expect("query blue sighting");
    assert!(s2.is_some(), "blue.png sighting should exist");

    let _ = std::fs::remove_dir_all(&temp_root);
}

fn imgseek_cli_index_args(path: &std::path::Path) -> imgseek::IndexArgs {
    imgseek::IndexArgs {
        paths: vec![path.to_path_buf()],
        corpus: Some("local:test_photos".to_string()),
        recursive: true,
        follow_symlinks: false,
        workers: Some(2),
        batch_size: 10,
        force_reindex: false,
        verbose: false,
    }
}

fn imgseek_run_index_direct(args: &imgseek::IndexArgs, config: &Config) -> anyhow::Result<()> {
    imgseek::run_index(args, config)
}
