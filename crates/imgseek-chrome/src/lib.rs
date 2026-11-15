pub mod discovery;
pub mod downloads;
pub mod error;
pub mod fake_profile;
pub mod history;
pub mod snapshot;

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
pub use snapshot::HistorySnapshot;
