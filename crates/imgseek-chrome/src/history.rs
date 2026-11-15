use crate::error::{ChromeError, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// Difference in seconds between WebKit epoch (1601-01-01) and Unix epoch (1970-01-01).
pub const WEBKIT_EPOCH_DIFF_SECS: i64 = 11_644_473_600;

/// Converts Chrome WebKit timestamp (microseconds since 1601-01-01 UTC) to Unix epoch seconds.
pub fn webkit_to_unix_secs(webkit_us: i64) -> i64 {
    if webkit_us <= 0 {
        return 0;
    }
    (webkit_us / 1_000_000).saturating_sub(WEBKIT_EPOCH_DIFF_SECS)
}

/// Converts Unix epoch seconds to Chrome WebKit timestamp (microseconds since 1601-01-01 UTC).
pub fn unix_secs_to_webkit(unix_secs: i64) -> i64 {
    if unix_secs <= 0 {
        return 0;
    }
    unix_secs.saturating_add(WEBKIT_EPOCH_DIFF_SECS) * 1_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeUrl {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub visit_count: i32,
    pub typed_count: i32,
    pub last_visit_time: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeVisit {
    pub id: i64,
    pub url_id: i64,
    pub visit_time: i64,
    pub from_visit: i64,
    pub transition: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeDownload {
    pub id: i64,
    pub target_path: String,
    pub start_time: i64,
    pub end_time: i64,
    pub received_bytes: i64,
    pub total_bytes: i64,
    pub state: i32,
    pub mime_type: Option<String>,
    pub tab_url: Option<String>,
    pub referrer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeDownloadUrlChain {
    pub id: i64,
    pub chain_index: i32,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeSearchTerm {
    pub keyword_id: i64,
    pub url_id: i64,
    pub lower_term: String,
    pub term: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChromeHistoryData {
    pub urls: Vec<ChromeUrl>,
    pub visits: Vec<ChromeVisit>,
    pub downloads: Vec<ChromeDownload>,
    pub download_chains: Vec<ChromeDownloadUrlChain>,
    pub search_terms: Vec<ChromeSearchTerm>,
}

pub struct HistoryReader;

impl HistoryReader {
    /// Opens and parses all relevant browsing history records from an isolated copy of `History`.
    pub fn read_file<P: AsRef<Path>>(db_path: P) -> Result<ChromeHistoryData> {
        let path = db_path.as_ref();
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| {
            ChromeError::LockedDatabase(format!("failed to open history database: {}", e))
        })?;

        conn.execute_batch("PRAGMA query_only = ON; PRAGMA busy_timeout = 5000;")
            .map_err(|e| ChromeError::LockedDatabase(format!("failed to set pragmas: {}", e)))?;

        let urls = Self::read_urls(&conn)?;
        let visits = Self::read_visits(&conn)?;
        let downloads = Self::read_downloads(&conn)?;
        let download_chains = Self::read_download_chains(&conn)?;
        let search_terms = Self::read_search_terms(&conn)?;

        Ok(ChromeHistoryData {
            urls,
            visits,
            downloads,
            download_chains,
            search_terms,
        })
    }

    fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
        let mut stmt = conn
            .prepare("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1")
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let exists = stmt
            .exists([table])
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        Ok(exists)
    }

    fn table_columns(conn: &Connection, table: &str) -> Result<HashSet<String>> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let mut cols = HashSet::new();
        for col in rows.flatten() {
            cols.insert(col.to_lowercase());
        }
        Ok(cols)
    }

    fn read_urls(conn: &Connection) -> Result<Vec<ChromeUrl>> {
        if !Self::table_exists(conn, "urls")? {
            return Ok(Vec::new());
        }

        let cols = Self::table_columns(conn, "urls")?;
        let has_title = cols.contains("title");
        let has_last_visit = cols.contains("last_visit_time");
        let has_visit_count = cols.contains("visit_count");
        let has_typed_count = cols.contains("typed_count");

        let query = format!(
            "SELECT id, url, {}, {}, {}, {} FROM urls",
            if has_title { "title" } else { "NULL" },
            if has_visit_count { "visit_count" } else { "0" },
            if has_typed_count { "typed_count" } else { "0" },
            if has_last_visit {
                "last_visit_time"
            } else {
                "0"
            }
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let url: String = row.get(1)?;
                let title: Option<String> = row.get(2)?;
                let visit_count: i32 = row.get(3)?;
                let typed_count: i32 = row.get(4)?;
                let raw_time: i64 = row.get(5)?;

                Ok(ChromeUrl {
                    id,
                    url,
                    title,
                    visit_count,
                    typed_count,
                    last_visit_time: webkit_to_unix_secs(raw_time),
                })
            })
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows.flatten() {
            out.push(r);
        }
        Ok(out)
    }

    fn read_visits(conn: &Connection) -> Result<Vec<ChromeVisit>> {
        if !Self::table_exists(conn, "visits")? {
            return Ok(Vec::new());
        }

        let cols = Self::table_columns(conn, "visits")?;
        let has_from = cols.contains("from_visit");
        let has_trans = cols.contains("transition");

        let query = format!(
            "SELECT id, url, visit_time, {}, {} FROM visits",
            if has_from { "from_visit" } else { "0" },
            if has_trans { "transition" } else { "0" }
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let url_id: i64 = row.get(1)?;
                let raw_time: i64 = row.get(2)?;
                let from_visit: i64 = row.get(3)?;
                let transition: i32 = row.get(4)?;

                Ok(ChromeVisit {
                    id,
                    url_id,
                    visit_time: webkit_to_unix_secs(raw_time),
                    from_visit,
                    transition,
                })
            })
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows.flatten() {
            out.push(r);
        }
        Ok(out)
    }

    fn read_downloads(conn: &Connection) -> Result<Vec<ChromeDownload>> {
        if !Self::table_exists(conn, "downloads")? {
            return Ok(Vec::new());
        }

        let cols = Self::table_columns(conn, "downloads")?;
        let path_col = if cols.contains("target_path") {
            "target_path"
        } else if cols.contains("full_path") {
            "full_path"
        } else {
            "''"
        };

        let has_end = cols.contains("end_time");
        let has_recv = cols.contains("received_bytes");
        let has_total = cols.contains("total_bytes");
        let has_state = cols.contains("state");
        let has_mime = cols.contains("mime_type");
        let has_tab = cols.contains("tab_url");
        let has_ref = cols.contains("referrer");

        let query = format!(
            "SELECT id, {}, start_time, {}, {}, {}, {}, {}, {}, {} FROM downloads",
            path_col,
            if has_end { "end_time" } else { "0" },
            if has_recv { "received_bytes" } else { "0" },
            if has_total { "total_bytes" } else { "0" },
            if has_state { "state" } else { "0" },
            if has_mime { "mime_type" } else { "NULL" },
            if has_tab { "tab_url" } else { "NULL" },
            if has_ref { "referrer" } else { "NULL" }
        );

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let target_path: String = row.get(1)?;
                let start_raw: i64 = row.get(2)?;
                let end_raw: i64 = row.get(3)?;
                let received_bytes: i64 = row.get(4)?;
                let total_bytes: i64 = row.get(5)?;
                let state: i32 = row.get(6)?;
                let mime_type: Option<String> = row.get(7)?;
                let tab_url: Option<String> = row.get(8)?;
                let referrer: Option<String> = row.get(9)?;

                Ok(ChromeDownload {
                    id,
                    target_path,
                    start_time: webkit_to_unix_secs(start_raw),
                    end_time: webkit_to_unix_secs(end_raw),
                    received_bytes,
                    total_bytes,
                    state,
                    mime_type,
                    tab_url,
                    referrer,
                })
            })
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows.flatten() {
            out.push(r);
        }
        Ok(out)
    }

    fn read_download_chains(conn: &Connection) -> Result<Vec<ChromeDownloadUrlChain>> {
        if !Self::table_exists(conn, "downloads_url_chains")? {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare(
                "SELECT id, chain_index, url FROM downloads_url_chains ORDER BY id, chain_index",
            )
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ChromeDownloadUrlChain {
                    id: row.get(0)?,
                    chain_index: row.get(1)?,
                    url: row.get(2)?,
                })
            })
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows.flatten() {
            out.push(r);
        }
        Ok(out)
    }

    fn read_search_terms(conn: &Connection) -> Result<Vec<ChromeSearchTerm>> {
        if !Self::table_exists(conn, "keyword_search_terms")? {
            return Ok(Vec::new());
        }

        let mut stmt = conn
            .prepare("SELECT keyword_id, url_id, lower_term, term FROM keyword_search_terms")
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ChromeSearchTerm {
                    keyword_id: row.get(0)?,
                    url_id: row.get(1)?,
                    lower_term: row.get(2)?,
                    term: row.get(3)?,
                })
            })
            .map_err(|e| ChromeError::LockedDatabase(e.to_string()))?;

        let mut out = Vec::new();
        for r in rows.flatten() {
            out.push(r);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webkit_unix_time_conversion() {
        // Known test point:
        // 2024-01-01 00:00:00 UTC = 1,704,067,200 unix seconds
        // WebKit time: (1,704,067,200 + 11,644,473,600) * 1,000,000 = 13,348,540,800,000,000 us
        let unix_sec = 1_704_067_200;
        let webkit_us = unix_secs_to_webkit(unix_sec);
        assert_eq!(webkit_us, 13_348_540_800_000_000);
        let back_to_unix = webkit_to_unix_secs(webkit_us);
        assert_eq!(back_to_unix, unix_sec);

        // Edge case: 0 or negative
        assert_eq!(webkit_to_unix_secs(0), 0);
        assert_eq!(webkit_to_unix_secs(-100), 0);
    }

    #[test]
    fn test_read_synthetic_history_db() {
        let tmp = std::env::temp_dir().join(format!(
            "synthetic_hist_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("History");

        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE urls (
                id INTEGER PRIMARY KEY,
                url LONGVARCHAR,
                title LONGVARCHAR,
                visit_count INTEGER DEFAULT 0,
                typed_count INTEGER DEFAULT 0,
                last_visit_time INTEGER
            );
            CREATE TABLE visits (
                id INTEGER PRIMARY KEY,
                url INTEGER,
                visit_time INTEGER,
                from_visit INTEGER,
                transition INTEGER
            );
            CREATE TABLE downloads (
                id INTEGER PRIMARY KEY,
                target_path LONGVARCHAR,
                start_time INTEGER,
                end_time INTEGER,
                received_bytes INTEGER,
                total_bytes INTEGER,
                state INTEGER,
                mime_type VARCHAR,
                tab_url LONGVARCHAR,
                referrer LONGVARCHAR
            );
            CREATE TABLE downloads_url_chains (
                id INTEGER,
                chain_index INTEGER,
                url LONGVARCHAR
            );
            CREATE TABLE keyword_search_terms (
                keyword_id INTEGER,
                url_id INTEGER,
                lower_term LONGVARCHAR,
                term LONGVARCHAR
            );
            "#,
        )
        .unwrap();

        let visit_time = unix_secs_to_webkit(1_700_000_000);
        conn.execute(
            "INSERT INTO urls (id, url, title, visit_count, typed_count, last_visit_time) VALUES (1, 'https://example.com/page', 'Example Page', 5, 2, ?1)",
            [visit_time],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO visits (id, url, visit_time, from_visit, transition) VALUES (10, 1, ?1, 0, 805306368)",
            [visit_time],
        )
        .unwrap();

        conn.execute(
            r#"INSERT INTO downloads (id, target_path, start_time, end_time, received_bytes, total_bytes, state, mime_type, tab_url, referrer)
               VALUES (100, '/path/to/download.jpg', ?1, ?1, 1024, 1024, 1, 'image/jpeg', 'https://example.com/page', 'https://example.com')"#,
            [visit_time],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO downloads_url_chains (id, chain_index, url) VALUES (100, 0, 'https://example.com/download.jpg')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO keyword_search_terms (keyword_id, url_id, lower_term, term) VALUES (1, 1, 'example search', 'Example Search')",
            [],
        )
        .unwrap();

        drop(conn);

        let data = HistoryReader::read_file(&db_path).unwrap();
        assert_eq!(data.urls.len(), 1);
        assert_eq!(data.urls[0].url, "https://example.com/page");
        assert_eq!(data.urls[0].title.as_deref(), Some("Example Page"));
        assert_eq!(data.urls[0].last_visit_time, 1_700_000_000);

        assert_eq!(data.visits.len(), 1);
        assert_eq!(data.visits[0].url_id, 1);
        assert_eq!(data.visits[0].visit_time, 1_700_000_000);

        assert_eq!(data.downloads.len(), 1);
        assert_eq!(data.downloads[0].target_path, "/path/to/download.jpg");
        assert_eq!(data.downloads[0].mime_type.as_deref(), Some("image/jpeg"));
        assert_eq!(data.downloads[0].start_time, 1_700_000_000);

        assert_eq!(data.download_chains.len(), 1);
        assert_eq!(
            data.download_chains[0].url,
            "https://example.com/download.jpg"
        );

        assert_eq!(data.search_terms.len(), 1);
        assert_eq!(data.search_terms[0].term, "Example Search");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_read_drifted_history_db_with_missing_columns() {
        let tmp = std::env::temp_dir().join(format!(
            "drifted_hist_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let db_path = tmp.join("History");

        // Schema without tab_url, referrer, or keyword_search_terms table
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE urls (
                id INTEGER PRIMARY KEY,
                url LONGVARCHAR
            );
            CREATE TABLE downloads (
                id INTEGER PRIMARY KEY,
                full_path LONGVARCHAR,
                start_time INTEGER
            );
            "#,
        )
        .unwrap();

        conn.execute(
            "INSERT INTO urls (id, url) VALUES (1, 'https://example.com/drift')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO downloads (id, full_path, start_time) VALUES (5, '/download/pic.png', 0)",
            [],
        )
        .unwrap();

        drop(conn);

        let data = HistoryReader::read_file(&db_path).unwrap();
        assert_eq!(data.urls.len(), 1);
        assert_eq!(data.urls[0].url, "https://example.com/drift");
        assert_eq!(data.urls[0].title, None);

        assert_eq!(data.downloads.len(), 1);
        assert_eq!(data.downloads[0].target_path, "/download/pic.png");
        assert_eq!(data.downloads[0].mime_type, None);

        assert!(data.visits.is_empty());
        assert!(data.search_terms.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
