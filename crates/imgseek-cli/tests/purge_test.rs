use imgseek::{run_chrome_index, run_index, run_purge, ChromeIndexArgs, IndexArgs, PurgeArgs};
use imgseek_chrome::FakeChromeProfile;
use imgseek_core::Config;
use imgseek_index::Manifest;
use imgseek_store::{BlobStore, Database};
use std::fs;

#[test]
fn test_cli_purge_corpus_e2e() {
    let tmp = std::env::temp_dir().join(format!(
        "cli_purge_test_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&tmp).unwrap();

    let user_data_dir = tmp.join("user_data");
    let local_photos_dir = tmp.join("photos");
    let index_dir = tmp.join("index");
    let blob_dir = tmp.join("blobs");
    fs::create_dir_all(&user_data_dir).unwrap();
    fs::create_dir_all(&local_photos_dir).unwrap();
    fs::create_dir_all(&index_dir).unwrap();
    fs::create_dir_all(&blob_dir).unwrap();

    let mut config = Config::default();
    config.ingest.min_dimension = 1;
    config.index.dir = index_dir.clone();
    config.index.blob_dir = blob_dir.clone();

    // 1. Create and index a local image to keep
    let keep_bmp_path = local_photos_dir.join("keep.bmp");
    // 2x2 green BMP
    let green_bmp: [u8; 70] = [
        0x42, 0x4d, 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 1,
        0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255,
        0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0,
    ];
    fs::write(&keep_bmp_path, green_bmp).unwrap();

    run_index(
        &IndexArgs {
            subcommand: None,
            paths: vec![local_photos_dir],
            corpus: Some("local:keep".into()),
            recursive: false,
            follow_symlinks: false,
            workers: Some(1),
            batch_size: 10,
            force_reindex: false,
            verbose: false,
        },
        &config,
    )
    .unwrap();

    // 2. Create Chrome profile and index downloaded image + cache image into chrome_history
    let profile = FakeChromeProfile::create(&user_data_dir, "Default").unwrap();
    let url_id = profile
        .add_url(
            "https://testnews.com/article",
            Some("Article"),
            1_700_000_000,
        )
        .unwrap();
    profile.add_visit(url_id, 1_700_000_000).unwrap();

    let chrome_dl_path = tmp.join("chrome_download.bmp");
    // 2x2 red BMP
    let red_bmp: [u8; 70] = [
        0x42, 0x4d, 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 1,
        0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0,
        0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0,
    ];
    fs::write(&chrome_dl_path, red_bmp).unwrap();

    profile
        .add_download(
            1,
            &chrome_dl_path,
            1_700_000_005,
            Some("image/bmp"),
            Some("https://testnews.com/article"),
            None,
        )
        .unwrap();

    run_chrome_index(
        &ChromeIndexArgs {
            user_data_dir: Some(user_data_dir),
            profile: "Default".into(),
            all_profiles: false,
            corpus: "chrome_history".into(),
            since: None,
            no_cache: true,
            no_downloads: false,
            exclude_patterns: vec![],
            verbose: false,
        },
        &config,
    )
    .unwrap();

    // Verify initial state: 2 corpora exist
    {
        let db = Database::open(index_dir.join("metadata.db")).unwrap();
        let chrome_docs = db.get_docs_by_corpus("chrome_history").unwrap();
        let keep_docs = db.get_docs_by_corpus("local:keep").unwrap();
        assert_eq!(chrome_docs.len(), 1);
        assert_eq!(keep_docs.len(), 1);

        let manifest = Manifest::load(&index_dir).unwrap();
        let total_indexed_docs: u32 = manifest.segments.iter().map(|s| s.doc_count).sum();
        assert_eq!(total_indexed_docs, 2);
    }

    // 3. Purge chrome_history
    run_purge(
        &PurgeArgs {
            corpus: "chrome_history".into(),
            no_compact: false,
            no_vacuum: false,
        },
        &config,
    )
    .unwrap();

    // 4. Verify post-purge guarantees
    {
        let db = Database::open(index_dir.join("metadata.db")).unwrap();
        let chrome_docs = db.get_docs_by_corpus("chrome_history").unwrap();
        assert_eq!(chrome_docs.len(), 0);

        let corpus_entry = db.get_corpus("chrome_history").unwrap();
        assert_eq!(corpus_entry, None);

        // local:keep must remain completely untouched
        let keep_docs = db.get_docs_by_corpus("local:keep").unwrap();
        assert_eq!(keep_docs.len(), 1);

        // Search index segments compacted: remaining segment has only the 1 retained doc
        let manifest = Manifest::load(&index_dir).unwrap();
        let total_indexed_docs: u32 = manifest.segments.iter().map(|s| s.doc_count).sum();
        assert_eq!(total_indexed_docs, 1);
        let total_tombstones: u32 = manifest.segments.iter().map(|s| s.tombstone_count).sum();
        assert_eq!(total_tombstones, 0);

        // Blob store check: unreferenced chrome image blob is deleted
        let blob_store = BlobStore::new(&blob_dir).unwrap();
        let red_chroma_hex = imgseek_store::bytes_to_hex(&imgseek_store::compute_sha256(
            &imgseek_core::compute_color_histogram(
                &imgseek_core::normalize(&red_bmp, &config.normalize)
                    .unwrap()
                    .analysis
                    .rgb,
            )
            .iter()
            .flat_map(|b| b.to_le_bytes())
            .collect::<Vec<u8>>(),
        ));
        assert!(!blob_store.has(&red_chroma_hex));
    }

    let _ = fs::remove_dir_all(&tmp);
}
