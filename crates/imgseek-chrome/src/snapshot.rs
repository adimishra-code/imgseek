use crate::error::{ChromeError, Result};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SNAPSHOT_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub struct HistorySnapshot {
    snapshot_dir: PathBuf,
    db_path: PathBuf,
}

impl HistorySnapshot {
    /// Creates a private, isolated copy of Chrome's `History` database (and active WAL/shm files).
    pub fn create<P: AsRef<Path>>(profile_path: P) -> Result<Self> {
        let profile = profile_path.as_ref();
        let history_src = if profile.is_file() {
            profile.to_path_buf()
        } else {
            profile.join("History")
        };

        if !history_src.exists() {
            return Err(ChromeError::ProfileNotFound(format!(
                "History database not found at {}",
                history_src.display()
            )));
        }

        let id = SNAPSHOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!(
            "imgseek_hist_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            id
        ));
        fs::create_dir_all(&temp_dir)?;

        let dest_history = temp_dir.join("History");

        // Copy primary History database
        if let Err(e) = copy_with_share_flags(&history_src, &dest_history) {
            let _ = fs::remove_dir_all(&temp_dir);
            if is_sharing_violation(&e) {
                return Err(ChromeError::LockedDatabase(
                    "History database is locked by Chrome. Close Chrome and retry, or use --copy-from <dir>".into(),
                ));
            }
            return Err(ChromeError::Io(e));
        }

        // Copy sibling WAL, SHM, and journal files if present
        let parent = history_src.parent().unwrap_or(profile);
        let siblings = ["History-wal", "History-shm", "History-journal"];
        for sibling_name in siblings {
            let src = parent.join(sibling_name);
            if src.exists() {
                let dest = temp_dir.join(sibling_name);
                let _ = copy_with_share_flags(&src, &dest);
            }
        }

        Ok(Self {
            snapshot_dir: temp_dir,
            db_path: dest_history,
        })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn snapshot_dir(&self) -> &Path {
        &self.snapshot_dir
    }
}

impl Drop for HistorySnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.snapshot_dir);
    }
}

/// Copies a file with permissive sharing flags to allow reading files currently open by Chrome.
fn copy_with_share_flags(src: &Path, dest: &Path) -> io::Result<u64> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ (1) | FILE_SHARE_WRITE (2) | FILE_SHARE_DELETE (4) = 7
        let mut opts = fs::OpenOptions::new();
        opts.read(true);
        opts.share_mode(7);

        let mut reader = opts.open(src)?;
        let mut writer = fs::File::create(dest)?;
        io::copy(&mut reader, &mut writer)
    }

    #[cfg(not(target_os = "windows"))]
    {
        fs::copy(src, dest)
    }
}

fn is_sharing_violation(err: &io::Error) -> bool {
    #[cfg(target_os = "windows")]
    {
        if let Some(raw_os_error) = err.raw_os_error() {
            // ERROR_SHARING_VIOLATION (32) or ERROR_LOCK_VIOLATION (33)
            return raw_os_error == 32 || raw_os_error == 33;
        }
    }
    err.kind() == io::ErrorKind::PermissionDenied
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snapshot_lifecycle() {
        let id = SNAPSHOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("chrome_snap_test_{}", id));
        fs::create_dir_all(&tmp).unwrap();

        let history_file = tmp.join("History");
        fs::write(&history_file, b"SQLite format 3\0fake database content").unwrap();

        let wal_file = tmp.join("History-wal");
        fs::write(&wal_file, b"fake wal data").unwrap();

        let snapshot = HistorySnapshot::create(&tmp).unwrap();
        assert!(snapshot.db_path().exists());
        assert_eq!(
            fs::read(snapshot.db_path()).unwrap(),
            b"SQLite format 3\0fake database content"
        );
        assert!(snapshot.snapshot_dir().join("History-wal").exists());

        let snapshot_dir = snapshot.snapshot_dir().to_path_buf();
        assert!(snapshot_dir.exists());

        // Dropping snapshot deletes temp dir
        drop(snapshot);
        assert!(!snapshot_dir.exists());

        // Cleanup
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_snapshot_missing_history_fails() {
        let id = SNAPSHOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = std::env::temp_dir().join(format!("chrome_snap_empty_{}", id));
        fs::create_dir_all(&tmp).unwrap();

        let err = HistorySnapshot::create(&tmp).unwrap_err();
        match err {
            ChromeError::ProfileNotFound(_) => {}
            other => panic!("expected ProfileNotFound, got {:?}", other),
        }

        let _ = fs::remove_dir_all(&tmp);
    }
}
