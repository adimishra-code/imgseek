pub mod association;
pub mod discovery;
pub mod downloads;
pub mod error;
pub mod fake_profile;
pub mod history;
pub mod simple_cache;
pub mod snapshot;

pub use association::{
    extract_host, ingest_cached_images, match_cache_entry_to_page, CacheIngestStats,
    PageMatchResult,
};
pub use discovery::{default_user_data_dir, find_all_profiles, find_profile, ChromeProfile};
pub use downloads::{
    ingest_downloaded_images, is_image_download, matches_exclude_patterns, DownloadIngestStats,
};
pub use error::{ChromeError, Result};
pub use fake_profile::FakeChromeProfile;
pub use history::{
    unix_secs_to_webkit, webkit_to_unix_secs, ChromeDownload, ChromeDownloadUrlChain,
    ChromeHistoryData, ChromeSearchTerm, ChromeUrl, ChromeVisit, HistoryReader,
    WEBKIT_EPOCH_DIFF_SECS,
};
pub use simple_cache::{
    parse_cache_entry_bytes, parse_cache_entry_file, parse_cache_item, parse_cache_key,
    parse_http_date, parse_http_headers, HttpHeaderInfo, ParsedCacheItem, RawCacheEntry,
    SimpleFileEOF, SimpleFileHeader, EOF_RECORD_SIZE, FLAG_HAS_CRC32, FLAG_HAS_KEY_SHA256,
    HEADER_SIZE, SIMPLE_FINAL_MAGIC, SIMPLE_INITIAL_MAGIC,
};
pub use snapshot::HistorySnapshot;
