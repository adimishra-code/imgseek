pub mod discovery;
pub mod error;
pub mod history;
pub mod snapshot;

pub use discovery::{default_user_data_dir, find_all_profiles, find_profile, ChromeProfile};
pub use error::{ChromeError, Result};
pub use history::{
    unix_secs_to_webkit, webkit_to_unix_secs, ChromeDownload, ChromeDownloadUrlChain,
    ChromeHistoryData, ChromeSearchTerm, ChromeUrl, ChromeVisit, HistoryReader,
    WEBKIT_EPOCH_DIFF_SECS,
};
pub use snapshot::HistorySnapshot;
