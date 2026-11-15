use crate::error::Result;
use crate::history::unix_secs_to_webkit;
use crc32fast::Hasher;
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

/// A synthetic Chrome profile generator for deterministic testing.
pub struct FakeChromeProfile {
    pub root_dir: PathBuf,
    pub profile_name: String,
    pub profile_dir: PathBuf,
    pub history_path: PathBuf,
    pub cache_dir: PathBuf,
}

impl FakeChromeProfile {
    /// Creates a new fake Chrome profile structure inside `root_dir` under `profile_name`.
    pub fn create(root_dir: &Path, profile_name: &str) -> Result<Self> {
        let profile_dir = root_dir.join(profile_name);
        let history_path = profile_dir.join("History");
        let cache_dir = profile_dir.join("Cache").join("Cache_Data");

        fs::create_dir_all(&cache_dir)?;

        let conn = Connection::open(&history_path)
            .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;

        conn.execute_batch(
            "CREATE TABLE urls (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL,
                title TEXT,
                visit_count INTEGER DEFAULT 0,
                typed_count INTEGER DEFAULT 0,
                last_visit_time INTEGER DEFAULT 0,
                hidden INTEGER DEFAULT 0
            );
            CREATE TABLE visits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url INTEGER NOT NULL,
                visit_time INTEGER NOT NULL,
                from_visit INTEGER,
                transition INTEGER DEFAULT 0,
                segment_id INTEGER DEFAULT 0,
                visit_duration INTEGER DEFAULT 0,
                incremented_visit_count INTEGER DEFAULT 1
            );
            CREATE TABLE downloads (
                id INTEGER PRIMARY KEY,
                guid TEXT,
                current_path TEXT,
                target_path TEXT,
                start_time INTEGER,
                received_bytes INTEGER,
                total_bytes INTEGER,
                state INTEGER,
                danger_type INTEGER,
                interrupt_reason INTEGER,
                raw_info BLOB,
                end_time INTEGER,
                opened INTEGER,
                last_access_time INTEGER,
                transient INTEGER,
                referrer TEXT,
                site_url TEXT,
                tab_url TEXT,
                tab_referrer_url TEXT,
                http_method TEXT,
                by_ext_id TEXT,
                by_ext_name TEXT,
                etag TEXT,
                last_modified TEXT,
                mime_type TEXT,
                original_mime_type TEXT
            );
            CREATE TABLE downloads_url_chains (
                id INTEGER NOT NULL,
                chain_index INTEGER NOT NULL,
                url TEXT NOT NULL,
                PRIMARY KEY (id, chain_index)
            );
            CREATE TABLE keyword_search_terms (
                keyword_id INTEGER NOT NULL,
                url_id INTEGER NOT NULL,
                lower_term TEXT NOT NULL,
                term TEXT NOT NULL
            );",
        )
        .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;

        // Write "Local State" file in root_dir so profile discovery identifies it
        let local_state_path = root_dir.join("Local State");
        if !local_state_path.exists() {
            let json = serde_json::json!({
                "profile": {
                    "info_cache": {
                        profile_name: {
                            "name": profile_name,
                            "is_using_default_name": true
                        }
                    }
                }
            });
            let _ = fs::write(local_state_path, json.to_string());
        }

        Ok(Self {
            root_dir: root_dir.to_path_buf(),
            profile_name: profile_name.to_string(),
            profile_dir,
            history_path,
            cache_dir,
        })
    }

    /// Adds a URL row to the fake profile's History database.
    pub fn add_url(
        &self,
        url: &str,
        title: Option<&str>,
        last_visit_unix_secs: i64,
    ) -> Result<i64> {
        let conn = Connection::open(&self.history_path)
            .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;
        let webkit_ts = unix_secs_to_webkit(last_visit_unix_secs);
        conn.execute(
            "INSERT INTO urls (url, title, visit_count, typed_count, last_visit_time, hidden)
             VALUES (?1, ?2, 1, 0, ?3, 0);",
            rusqlite::params![url, title, webkit_ts],
        )
        .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;
        Ok(conn.last_insert_rowid())
    }

    /// Adds a visit row to the fake profile's History database.
    pub fn add_visit(&self, url_id: i64, visit_unix_secs: i64) -> Result<i64> {
        let conn = Connection::open(&self.history_path)
            .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;
        let webkit_ts = unix_secs_to_webkit(visit_unix_secs);
        conn.execute(
            "INSERT INTO visits (url, visit_time, from_visit, transition, segment_id, visit_duration, incremented_visit_count)
             VALUES (?1, ?2, 0, 0, 0, 0, 1);",
            rusqlite::params![url_id, webkit_ts],
        )
        .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;
        Ok(conn.last_insert_rowid())
    }

    /// Adds a download row to the fake profile's History database.
    pub fn add_download(
        &self,
        id: i64,
        target_path: &Path,
        start_time_unix_secs: i64,
        mime_type: Option<&str>,
        tab_url: Option<&str>,
        referrer: Option<&str>,
    ) -> Result<()> {
        let conn = Connection::open(&self.history_path)
            .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;
        let webkit_ts = unix_secs_to_webkit(start_time_unix_secs);
        let path_str = target_path.to_string_lossy().to_string();
        let bytes_len = if target_path.exists() {
            fs::metadata(target_path)
                .map(|m| m.len() as i64)
                .unwrap_or(0)
        } else {
            0
        };

        conn.execute(
            "INSERT INTO downloads (
                id, guid, current_path, target_path, start_time, received_bytes, total_bytes,
                state, danger_type, interrupt_reason, end_time, opened, last_access_time,
                transient, referrer, tab_url, mime_type
             ) VALUES (
                ?1, 'guid', ?2, ?3, ?4, ?5, ?6,
                1, 0, 0, ?4, 0, ?4,
                0, ?7, ?8, ?9
             );",
            rusqlite::params![
                id, path_str, path_str, webkit_ts, bytes_len, bytes_len, referrer, tab_url,
                mime_type,
            ],
        )
        .map_err(|e| crate::error::ChromeError::LockedDatabase(e.to_string()))?;
        Ok(())
    }

    /// Adds a binary Simple Cache `_0` entry file into the profile's `Cache/Cache_Data/` directory.
    pub fn add_cache_entry(
        &self,
        key: &str,
        stream1_body: &[u8],
        content_type: Option<&str>,
        date_header: Option<&str>,
    ) -> Result<PathBuf> {
        let mut key_hasher = Hasher::new();
        key_hasher.update(key.as_bytes());
        let key_hash = key_hasher.finalize();

        let filename = format!("{:016x}_0", key_hash as u64);
        let entry_path = self.cache_dir.join(&filename);

        // Build stream 0 (headers)
        let ct = content_type.unwrap_or("image/jpeg");
        let date = date_header.unwrap_or("Sun, 15 Nov 2026 12:00:00 GMT");
        let headers_str = format!("HTTP/1.1 200 OK\0Content-Type: {}\0Date: {}\0\0", ct, date);
        let stream0_bytes = headers_str.as_bytes();

        let mut s0_hasher = Hasher::new();
        s0_hasher.update(stream0_bytes);
        let s0_crc = s0_hasher.finalize();

        let mut s1_hasher = Hasher::new();
        s1_hasher.update(stream1_body);
        let s1_crc = s1_hasher.finalize();

        let mut file_bytes = Vec::new();

        // 1. SimpleFileHeader (24 bytes)
        file_bytes.extend_from_slice(&0xfcfb6d1ba7725c30u64.to_le_bytes()); // magic
        file_bytes.extend_from_slice(&8u32.to_le_bytes()); // version
        file_bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
        file_bytes.extend_from_slice(&key_hash.to_le_bytes());
        file_bytes.extend_from_slice(&0u32.to_le_bytes()); // padding

        // 2. Key bytes
        file_bytes.extend_from_slice(key.as_bytes());

        // 3. Stream 1 Data (Image body)
        file_bytes.extend_from_slice(stream1_body);

        // 4. SimpleFileEOF for Stream 1 (24 bytes)
        file_bytes.extend_from_slice(&0xf4fa6f45970d41d8u64.to_le_bytes());
        file_bytes.extend_from_slice(&1u32.to_le_bytes()); // FLAG_HAS_CRC32
        file_bytes.extend_from_slice(&s1_crc.to_le_bytes());
        file_bytes.extend_from_slice(&(stream1_body.len() as u32).to_le_bytes());
        file_bytes.extend_from_slice(&0u32.to_le_bytes()); // padding

        // 5. Stream 0 Data (HTTP headers)
        file_bytes.extend_from_slice(stream0_bytes);

        // 6. SimpleFileEOF for Stream 0 (24 bytes)
        file_bytes.extend_from_slice(&0xf4fa6f45970d41d8u64.to_le_bytes());
        file_bytes.extend_from_slice(&1u32.to_le_bytes()); // FLAG_HAS_CRC32
        file_bytes.extend_from_slice(&s0_crc.to_le_bytes());
        file_bytes.extend_from_slice(&(stream0_bytes.len() as u32).to_le_bytes());
        file_bytes.extend_from_slice(&0u32.to_le_bytes()); // padding

        fs::write(&entry_path, file_bytes)?;
        Ok(entry_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fake_profile_creation_and_population() {
        let tmp = std::env::temp_dir().join(format!(
            "fake_profile_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();

        let profile = FakeChromeProfile::create(&tmp, "Default").unwrap();
        assert!(profile.history_path.exists());
        assert!(profile.cache_dir.exists());

        let url_id = profile
            .add_url("https://example.com/test", Some("Test Page"), 1_700_000_000)
            .unwrap();
        assert_eq!(url_id, 1);

        let visit_id = profile.add_visit(url_id, 1_700_000_005).unwrap();
        assert_eq!(visit_id, 1);

        let sample_dl_path = tmp.join("download.png");
        fs::write(&sample_dl_path, b"dummy png").unwrap();
        profile
            .add_download(
                1,
                &sample_dl_path,
                1_700_000_010,
                Some("image/png"),
                Some("https://example.com/test"),
                None,
            )
            .unwrap();

        let cache_file = profile
            .add_cache_entry(
                "https://example.com/images/cat.jpg",
                b"fake image bytes",
                Some("image/jpeg"),
                None,
            )
            .unwrap();
        assert!(cache_file.exists());
        assert!(cache_file
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("_0"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
