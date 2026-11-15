pub mod discovery;
pub mod error;
pub mod snapshot;

pub use discovery::{default_user_data_dir, find_all_profiles, find_profile, ChromeProfile};
pub use error::{ChromeError, Result};
pub use snapshot::HistorySnapshot;
