use imgseek_store::{Corpus, Database, NewDoc, NewSighting};

#[test]
fn test_schema_creation_and_pragmas() {
    let db = Database::open_in_memory().expect("open database in memory");
    let conn = db.conn();

    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys;", [], |r| r.get(0))
        .expect("query foreign_keys");
    assert_eq!(fk, 1);

    let busy: i64 = conn
        .query_row("PRAGMA busy_timeout;", [], |r| r.get(0))
        .expect("query busy_timeout");
    assert_eq!(busy, 5000);

    let version: i64 = conn
        .query_row("PRAGMA user_version;", [], |r| r.get(0))
        .expect("query user_version");
    assert_eq!(version, 1);
}

#[test]
fn test_crud_and_foreign_keys() {
    let db = Database::open_in_memory().expect("open database");

    let corpus = Corpus {
        corpus_id: "local:test".to_string(),
        kind: "local".to_string(),
        root_uri: "/path/to/photos".to_string(),
        created_at: 1700000000,
    };
    db.insert_corpus(&corpus).expect("insert corpus");

    let fetched_corpus = db
        .get_corpus("local:test")
        .expect("get corpus")
        .expect("corpus exists");
    assert_eq!(fetched_corpus, corpus);

    let new_doc = NewDoc {
        sha256: [42u8; 32],
        byte_size: 1024,
        format: "jpeg".to_string(),
        width: 800,
        height: 600,
        ahash: 0x0123456789abcdef,
        dhash: 0xfedcba9876543210,
        phash: 0x1122334455667788,
        whash: 0x99aabbccddeeff00,
        chroma_blob_ref: "cas://chroma/1".to_string(),
        keypoints_ref: None,
        indexed_at: 1700000001,
    };
    let doc_id = db.insert_doc(&new_doc).expect("insert doc");
    assert!(doc_id > 0);

    let doc = db
        .get_doc_by_id(doc_id)
        .expect("get doc")
        .expect("doc exists");
    assert_eq!(doc.doc_id, doc_id);
    assert_eq!(doc.sha256, [42u8; 32]);
    assert_eq!(doc.ahash, 0x0123456789abcdef);
    assert_eq!(doc.whash, 0x99aabbccddeeff00);

    let doc_by_sha = db
        .get_doc_by_sha256(&[42u8; 32])
        .expect("get doc by sha256")
        .expect("doc exists");
    assert_eq!(doc_by_sha.doc_id, doc_id);

    let sighting = NewSighting {
        doc_id,
        corpus_id: "local:test".to_string(),
        origin_uri: "file:///path/to/photos/img1.jpg".to_string(),
        title: Some("img1.jpg".to_string()),
        discovered_at: 1700000002,
        file_size: Some(1024),
        file_mtime: Some(1700000000),
        http_etag: None,
        http_last_modified: None,
        extra_json: None,
    };
    let sighting_id = db
        .insert_or_replace_sighting(&sighting)
        .expect("insert sighting");
    assert!(sighting_id > 0);

    let fetched_sighting = db
        .get_sighting("local:test", "file:///path/to/photos/img1.jpg")
        .expect("get sighting")
        .expect("sighting exists");
    assert_eq!(fetched_sighting.sighting_id, sighting_id);
    assert_eq!(fetched_sighting.title.as_deref(), Some("img1.jpg"));

    let bad_sighting = NewSighting {
        doc_id: 999999,
        corpus_id: "local:test".to_string(),
        origin_uri: "file:///path/to/photos/bad.jpg".to_string(),
        title: None,
        discovered_at: 1700000002,
        file_size: None,
        file_mtime: None,
        http_etag: None,
        http_last_modified: None,
        extra_json: None,
    };
    assert!(db.insert_or_replace_sighting(&bad_sighting).is_err());
}

#[test]
fn test_index_state_storage() {
    let db = Database::open_in_memory().expect("open database");

    assert_eq!(db.get_index_state("last_scan").expect("query"), None);

    db.set_index_state("last_scan", "1700001234")
        .expect("set state");
    assert_eq!(
        db.get_index_state("last_scan").expect("query"),
        Some("1700001234".to_string())
    );

    db.set_index_state("last_scan", "1700005678")
        .expect("update state");
    assert_eq!(
        db.get_index_state("last_scan").expect("query"),
        Some("1700005678".to_string())
    );
}

#[test]
fn test_file_persistence_and_reopen() {
    let dir_name = format!(
        "imgseek_store_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_dir = std::env::temp_dir().join(dir_name);
    std::fs::create_dir_all(&temp_dir).expect("create temp dir");
    let db_path = temp_dir.join("metadata.db");

    {
        let db = Database::open(&db_path).expect("create db file");
        let corpus = Corpus {
            corpus_id: "test:corpus".to_string(),
            kind: "local".to_string(),
            root_uri: "/tmp".to_string(),
            created_at: 100,
        };
        db.insert_corpus(&corpus).expect("insert");
    }

    {
        let db = Database::open(&db_path).expect("reopen db file");
        let corpus = db.get_corpus("test:corpus").expect("query").expect("found");
        assert_eq!(corpus.created_at, 100);
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}
