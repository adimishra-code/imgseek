use imgseek::{run_chrome_index, run_chrome_report, ChromeIndexArgs, ChromeReportArgs};
use imgseek_chrome::FakeChromeProfile;
use imgseek_core::Config;
use imgseek_store::Database;
use std::fs;

#[test]
fn test_cli_chrome_index_and_report() {
    let tmp = std::env::temp_dir().join(format!(
        "cli_chrome_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&tmp).unwrap();

    let user_data_dir = tmp.join("user_data");
    let index_dir = tmp.join("index");
    let blob_dir = tmp.join("blobs");
    fs::create_dir_all(&user_data_dir).unwrap();
    fs::create_dir_all(&index_dir).unwrap();
    fs::create_dir_all(&blob_dir).unwrap();

    let mut config = Config::default();
    config.index.dir = index_dir.clone();
    config.index.blob_dir = blob_dir;

    let profile = FakeChromeProfile::create(&user_data_dir, "Default").unwrap();
    let url_id = profile
        .add_url(
            "https://testnews.com/tech/rust.html",
            Some("Rust News"),
            1_700_000_000,
        )
        .unwrap();
    profile.add_visit(url_id, 1_700_000_000).unwrap();

    // 2x2 BMP
    let bmp_bytes: [u8; 70] = [
        0x42, 0x4d, 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 1,
        0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0,
        0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0,
    ];

    // Downloaded image
    let dl_path = tmp.join("rust_logo.bmp");
    fs::write(&dl_path, bmp_bytes).unwrap();
    profile
        .add_download(
            1,
            &dl_path,
            1_700_000_005,
            Some("image/bmp"),
            Some("https://testnews.com/tech/rust.html"),
            Some("https://testnews.com/"),
        )
        .unwrap();

    // Cached image from same domain within window
    profile
        .add_cache_entry(
            "_dk_https://testnews.com https://testnews.com https://cdn.testnews.com/rust.bmp",
            &bmp_bytes,
            Some("image/bmp"),
            Some("Sun, 15 Nov 2026 12:00:00 GMT"),
        )
        .unwrap();

    let index_args = ChromeIndexArgs {
        user_data_dir: Some(user_data_dir),
        profile: "Default".into(),
        all_profiles: false,
        corpus: "chrome_history".into(),
        since: None,
        no_cache: false,
        no_downloads: false,
        exclude_patterns: vec![],
        verbose: true,
    };

    run_chrome_index(&index_args, &config).unwrap();

    // Verify DB entries
    let db = Database::open(index_dir.join("metadata.db")).unwrap();
    let docs = db.get_docs_by_corpus("chrome_history").unwrap();
    assert_eq!(docs.len(), 1); // Exact same 2x2 BMP byte content deduplicated to 1 doc

    let sightings = db.get_sightings_by_doc_id(docs[0].doc_id).unwrap();
    assert_eq!(sightings.len(), 2); // 1 download sighting + 1 cache sighting

    // Run report
    let report_args = ChromeReportArgs {
        corpus: "chrome_history".into(),
    };
    run_chrome_report(&report_args, &config).unwrap();

    let _ = fs::remove_dir_all(&tmp);
}
