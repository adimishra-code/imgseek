use image::{ImageBuffer, Rgb};
use imgseek::{run_index, run_query, IndexArgs, QueryArgs};
use imgseek_core::Config;

#[test]
fn test_query_command_exact_match_and_json() {
    let test_id = format!(
        "imgseek_qry_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_root = std::env::temp_dir().join(test_id);
    let images_dir = temp_root.join("images");
    let index_dir = temp_root.join("index");
    let blob_dir = temp_root.join("blobs");

    std::fs::create_dir_all(&images_dir).expect("create images dir");
    std::fs::create_dir_all(&index_dir).expect("create index dir");
    std::fs::create_dir_all(&blob_dir).expect("create blob dir");

    // Create 3 distinct images: red, blue, and a gradient
    let red_img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(100, 100, Rgb([255, 0, 0]));
    let red_path = images_dir.join("red.png");
    red_img.save(&red_path).expect("save red");

    let blue_img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(100, 100, Rgb([0, 0, 255]));
    let blue_path = images_dir.join("blue.png");
    blue_img.save(&blue_path).expect("save blue");

    let mut grad_img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(100, 100);
    for (x, y, pixel) in grad_img.enumerate_pixels_mut() {
        *pixel = Rgb([(x * 2) as u8, (y * 2) as u8, 128]);
    }
    let grad_path = images_dir.join("grad.png");
    grad_img.save(&grad_path).expect("save grad");

    let mut config = Config::default();
    config.index.dir = index_dir.clone();
    config.index.blob_dir = blob_dir.clone();

    // Index the directory
    let index_args = IndexArgs {
        paths: vec![images_dir.clone()],
        corpus: Some("local:test".to_string()),
        recursive: false,
        follow_symlinks: false,
        workers: Some(1),
        batch_size: 10,
        force_reindex: false,
        verbose: false,
    };
    run_index(&index_args, &config).expect("index");

    // Query with red image
    let query_args = QueryArgs {
        image_path: red_path.clone(),
        limit: 10,
        min_score: Some(0.55),
        json: true,
        corpus: None,
    };
    let results = run_query(&query_args, &config).expect("query red");

    assert!(!results.is_empty(), "Should find at least 1 match");
    let top_match = &results[0];
    assert_eq!(top_match.rank, 1);
    assert!(top_match.origin_uri.ends_with("red.png"));
    assert!(
        (top_match.score - 1.0).abs() < 1e-4,
        "Exact copy should score 1.0"
    );

    // Check explanation breakdown
    let explanation = &top_match.explanation;
    assert_eq!(explanation.tier, 1);
    assert_eq!(explanation.algorithm, "perceptual_hash_ensemble");
    assert!(explanation.match_status);
    assert_eq!(explanation.breakdown.phash_distance, 0);
    assert_eq!(explanation.breakdown.whash_distance, 0);
    assert_eq!(explanation.breakdown.dhash_distance, 0);
    assert_eq!(explanation.breakdown.ahash_distance, 0);
    assert!(explanation.breakdown.chroma_distance < 1e-4);

    // Query with blue image
    let query_blue = QueryArgs {
        image_path: blue_path.clone(),
        limit: 5,
        min_score: Some(0.55),
        json: false,
        corpus: None,
    };
    let blue_results = run_query(&query_blue, &config).expect("query blue");
    assert!(!blue_results.is_empty());
    assert_eq!(blue_results[0].rank, 1);
    assert!(blue_results[0].origin_uri.ends_with("blue.png"));

    // Query with non-matching image at high threshold
    let yellow_img: ImageBuffer<Rgb<u8>, Vec<u8>> =
        ImageBuffer::from_pixel(100, 100, Rgb([255, 255, 0]));
    let yellow_path = temp_root.join("yellow.png");
    yellow_img.save(&yellow_path).expect("save yellow");

    let query_yellow = QueryArgs {
        image_path: yellow_path,
        limit: 5,
        min_score: Some(0.95),
        json: false,
        corpus: None,
    };
    let yellow_results = run_query(&query_yellow, &config).expect("query yellow high threshold");
    assert!(
        yellow_results.is_empty(),
        "Unrelated yellow image should not match at 0.95"
    );

    let _ = std::fs::remove_dir_all(&temp_root);
}
