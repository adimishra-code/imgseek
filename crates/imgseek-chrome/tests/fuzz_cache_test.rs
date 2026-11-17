use imgseek_chrome::{
    parse_cache_entry_bytes, parse_cache_item, parse_cache_key, parse_http_date,
    parse_http_headers, FakeChromeProfile, HistoryReader,
};
use proptest::prelude::*;
use std::fs;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn fuzz_cache_parser_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let res = parse_cache_entry_bytes(&bytes);
        if let Ok(raw) = res {
            let _ = parse_cache_item(&raw);
        }
    }

    #[test]
    fn fuzz_cache_key_parser(key in "\\PC*") {
        let (resource, top_site) = parse_cache_key(&key);
        let _ = resource.len();
        if let Some(top) = top_site {
            let _ = top.len();
        }
    }

    #[test]
    fn fuzz_http_header_parser(header_bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let info = parse_http_headers(&header_bytes);
        let _ = info.content_type;
        let _ = info.date_timestamp;
    }

    #[test]
    fn fuzz_http_date_parser(date_str in "\\PC*") {
        let _ = parse_http_date(&date_str);
    }
}

#[test]
fn test_fuzz_cache_mutation_from_valid_entry() {
    let tmp = std::env::temp_dir().join(format!(
        "fuzz_cache_mut_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&tmp).unwrap();

    let profile = FakeChromeProfile::create(&tmp, "Default").unwrap();
    let key = "_dk_https://example.com https://example.com https://cdn.example.com/photo.jpg";
    let body = vec![42u8; 256];
    let path = profile
        .add_cache_entry(
            key,
            &body,
            Some("image/jpeg"),
            Some("Sun, 15 Nov 2026 12:00:00 GMT"),
        )
        .unwrap();

    let valid_bytes = fs::read(&path).unwrap();
    assert!(parse_cache_entry_bytes(&valid_bytes).is_ok());

    // Systematically mutate every single byte in the entry
    for offset in 0..valid_bytes.len() {
        let mut corrupted = valid_bytes.clone();
        corrupted[offset] ^= 0xff;
        let _ = parse_cache_entry_bytes(&corrupted);
    }

    // Truncate at every possible length from 0 to full length
    for len in 0..valid_bytes.len() {
        let truncated = &valid_bytes[..len];
        let _ = parse_cache_entry_bytes(truncated);
    }

    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn test_fuzz_corrupt_history_db() {
    let tmp = std::env::temp_dir().join(format!(
        "fuzz_hist_mut_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&tmp).unwrap();

    let db_path = tmp.join("History");
    fs::write(&db_path, b"not a valid sqlite database file").unwrap();

    let res = HistoryReader::read_file(&db_path);
    assert!(res.is_err());

    let _ = fs::remove_dir_all(&tmp);
}
