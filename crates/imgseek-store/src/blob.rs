use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::error::{Result, StoreError};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct BlobStore {
    root: PathBuf,
    tmp_dir: PathBuf,
    compression_level: i32,
}

impl BlobStore {
    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let tmp_dir = root.join(".tmp");
        std::fs::create_dir_all(&tmp_dir)?;
        Ok(Self {
            root,
            tmp_dir,
            compression_level: 3,
        })
    }

    pub fn with_compression_level<P: AsRef<Path>>(root: P, level: i32) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let tmp_dir = root.join(".tmp");
        std::fs::create_dir_all(&tmp_dir)?;
        Ok(Self {
            root,
            tmp_dir,
            compression_level: level,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, data: &[u8]) -> Result<String> {
        let hash = compute_sha256(data);
        let hash_hex = bytes_to_hex(&hash);
        let target_path = self.blob_path(&hash_hex)?;

        if target_path.exists() {
            return Ok(hash_hex);
        }

        let tmp_file = self.temp_path();
        {
            let mut file = std::fs::File::create(&tmp_file)?;
            zstd::stream::copy_encode(data, &mut file, self.compression_level)?;
            use std::io::Write;
            file.flush()?;
            file.sync_all()?;
        }

        if target_path.exists() {
            let _ = std::fs::remove_file(&tmp_file);
            return Ok(hash_hex);
        }

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        match std::fs::rename(&tmp_file, &target_path) {
            Ok(()) => Ok(hash_hex),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp_file);
                if target_path.exists() {
                    Ok(hash_hex)
                } else {
                    Err(StoreError::Io(e))
                }
            }
        }
    }

    pub fn get(&self, hash_hex: &str) -> Result<Vec<u8>> {
        let path = self.blob_path(hash_hex)?;
        if !path.is_file() {
            return Err(StoreError::BlobNotFound(hash_hex.to_string()));
        }

        let compressed = std::fs::read(&path)?;
        let decompressed = zstd::decode_all(compressed.as_slice())?;

        let actual_hash = bytes_to_hex(&compute_sha256(&decompressed));
        let expected_hash = hash_hex.to_ascii_lowercase();

        if actual_hash != expected_hash {
            return Err(StoreError::BlobCorrupted {
                expected: expected_hash,
                actual: actual_hash,
            });
        }

        Ok(decompressed)
    }

    pub fn has(&self, hash_hex: &str) -> bool {
        match self.blob_path(hash_hex) {
            Ok(p) => p.is_file(),
            Err(_) => false,
        }
    }

    pub fn delete(&self, hash_hex: &str) -> Result<bool> {
        let path = self.blob_path(hash_hex)?;
        if path.is_file() {
            std::fs::remove_file(&path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn blob_path(&self, hash_hex: &str) -> Result<PathBuf> {
        validate_hash_hex(hash_hex)?;
        let lower = hash_hex.to_ascii_lowercase();
        Ok(self.root.join(&lower[0..2]).join(&lower[2..4]).join(&lower))
    }

    fn temp_path(&self) -> PathBuf {
        let pid = std::process::id();
        let count = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        self.tmp_dir
            .join(format!("{}_{}_{}.tmp", pid, count, nanos))
    }
}

pub fn compute_sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

pub fn validate_hash_hex(hash_hex: &str) -> Result<()> {
    if hash_hex.len() != 64 || !hash_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(StoreError::InvalidBlobHash(hash_hex.to_string()));
    }
    Ok(())
}
