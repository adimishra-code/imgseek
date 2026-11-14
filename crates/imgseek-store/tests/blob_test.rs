use imgseek_store::{BlobStore, StoreError};

#[test]
fn test_blob_store_put_and_get() {
    let dir_name = format!(
        "imgseek_blob_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_dir = std::env::temp_dir().join(dir_name);
    let store = BlobStore::new(&temp_dir).expect("create store");

    let payload = b"The quick brown fox jumps over the lazy dog. Repeat for length! ".repeat(100);
    let hash = store.put(&payload).expect("put blob");

    assert_eq!(hash.len(), 64);
    assert!(store.has(&hash));

    // Check directory layout: <root>/ab/cd/<hash>
    let expected_path = temp_dir.join(&hash[0..2]).join(&hash[2..4]).join(&hash);
    assert!(expected_path.is_file());

    // Check raw file starts with zstd magic number (0x28, 0xB5, 0x2F, 0xFD in little-endian)
    let raw_bytes = std::fs::read(&expected_path).expect("read raw compressed file");
    assert!(raw_bytes.len() >= 4);
    assert_eq!(&raw_bytes[0..4], &[0x28, 0xB5, 0x2F, 0xFD]);

    // Read back and compare
    let read_payload = store.get(&hash).expect("get blob");
    assert_eq!(read_payload, payload);

    // Idempotent put
    let hash2 = store.put(&payload).expect("second put");
    assert_eq!(hash2, hash);

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_blob_store_errors() {
    let dir_name = format!(
        "imgseek_blob_err_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let temp_dir = std::env::temp_dir().join(dir_name);
    let store = BlobStore::new(&temp_dir).expect("create store");

    let nonexistent_hash = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    match store.get(nonexistent_hash) {
        Err(StoreError::BlobNotFound(h)) => assert_eq!(h, nonexistent_hash),
        other => panic!("expected BlobNotFound, got {:?}", other),
    }

    match store.get("not-a-valid-hex-hash") {
        Err(StoreError::InvalidBlobHash(_)) => {}
        other => panic!("expected InvalidBlobHash, got {:?}", other),
    }

    // Tampered data should trigger BlobCorrupted
    let payload = b"Original content to be tampered with";
    let hash = store.put(payload).expect("put blob");
    let blob_path = store.blob_path(&hash).expect("path");

    // Re-compress different data and write directly to the target path
    let tampered_payload = b"Tampered different content";
    let tampered_compressed = zstd::encode_all(&tampered_payload[..], 3).expect("encode");
    std::fs::write(&blob_path, tampered_compressed).expect("overwrite");

    match store.get(&hash) {
        Err(StoreError::BlobCorrupted { expected, actual }) => {
            assert_eq!(expected, hash);
            assert_ne!(actual, hash);
        }
        other => panic!("expected BlobCorrupted, got {:?}", other),
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}
