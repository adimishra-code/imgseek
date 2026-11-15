use crate::error::{ChromeError, Result};
use crc32fast::Hasher;
use std::fs;
use std::path::Path;

pub const SIMPLE_INITIAL_MAGIC: u64 = 0xfcfb6d1ba7725c30;
pub const SIMPLE_FINAL_MAGIC: u64 = 0xf4fa6f45970d41d8;
pub const HEADER_SIZE: usize = 24;
pub const EOF_RECORD_SIZE: usize = 24;

pub const FLAG_HAS_CRC32: u32 = 1 << 0;
pub const FLAG_HAS_KEY_SHA256: u32 = 1 << 1;

/// Represents the fixed 24-byte header at the start of a Chromium Simple Cache entry file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleFileHeader {
    pub initial_magic: u64,
    pub version: u32,
    pub key_length: u32,
    pub key_hash: u32,
}

impl SimpleFileHeader {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_SIZE {
            return Err(ChromeError::CorruptCacheEntry(
                "header buffer too small".into(),
            ));
        }

        let magic = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        if magic != SIMPLE_INITIAL_MAGIC {
            return Err(ChromeError::CorruptCacheEntry(format!(
                "invalid initial magic: 0x{:016x}, expected 0x{:016x}",
                magic, SIMPLE_INITIAL_MAGIC
            )));
        }

        let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let key_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let key_hash = u32::from_le_bytes(bytes[16..20].try_into().unwrap());

        Ok(Self {
            initial_magic: magic,
            version,
            key_length,
            key_hash,
        })
    }
}

/// Represents the fixed 24-byte EOF record appended after a stream in a Simple Cache entry file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleFileEOF {
    pub final_magic: u64,
    pub flags: u32,
    pub data_crc32: u32,
    pub stream_size: u32,
}

impl SimpleFileEOF {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < EOF_RECORD_SIZE {
            return Err(ChromeError::CorruptCacheEntry(
                "EOF record buffer too small".into(),
            ));
        }

        let magic = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        if magic != SIMPLE_FINAL_MAGIC {
            return Err(ChromeError::CorruptCacheEntry(format!(
                "invalid EOF magic: 0x{:016x}, expected 0x{:016x}",
                magic, SIMPLE_FINAL_MAGIC
            )));
        }

        let flags = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let data_crc32 = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
        let stream_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap());

        Ok(Self {
            final_magic: magic,
            flags,
            data_crc32,
            stream_size,
        })
    }

    pub fn has_crc32(&self) -> bool {
        (self.flags & FLAG_HAS_CRC32) != 0
    }

    pub fn has_key_sha256(&self) -> bool {
        (self.flags & FLAG_HAS_KEY_SHA256) != 0
    }
}

/// Low-level parsed representation of a Chromium Simple Cache `_0` entry.
#[derive(Debug, Clone)]
pub struct RawCacheEntry {
    pub key: String,
    pub stream0_headers: Vec<u8>,
    pub stream1_payload: Vec<u8>,
}

/// Parses a Chromium Simple Cache `_0` entry file from a raw byte slice.
pub fn parse_cache_entry_bytes(file_bytes: &[u8]) -> Result<RawCacheEntry> {
    // Minimum valid size: Header (24) + EOF1 (24) + EOF0 (24) = 72 bytes
    if file_bytes.len() < HEADER_SIZE + (2 * EOF_RECORD_SIZE) {
        return Err(ChromeError::CorruptCacheEntry(format!(
            "cache file size {} bytes is less than minimum 72 bytes",
            file_bytes.len()
        )));
    }

    let file_len = file_bytes.len();

    // 1. Parse Stream 0 EOF at the very end of the file
    let s0_eof_start = file_len - EOF_RECORD_SIZE;
    let s0_eof = SimpleFileEOF::parse(&file_bytes[s0_eof_start..file_len])?;

    // 2. Check for optional 32-byte key SHA256 preceding Stream 0 EOF
    let key_sha256_len = if s0_eof.has_key_sha256() { 32 } else { 0 };

    let s0_size = s0_eof.stream_size as usize;
    let s0_end = s0_eof_start
        .checked_sub(key_sha256_len)
        .ok_or_else(|| ChromeError::CorruptCacheEntry("invalid key sha256 offset".into()))?;
    let s0_start = s0_end
        .checked_sub(s0_size)
        .ok_or_else(|| ChromeError::CorruptCacheEntry("stream 0 offset underflow".into()))?;

    if s0_start < HEADER_SIZE + EOF_RECORD_SIZE {
        return Err(ChromeError::CorruptCacheEntry(
            "stream 0 offset exceeds file boundaries".into(),
        ));
    }

    let stream0_data = &file_bytes[s0_start..s0_end];
    if s0_eof.has_crc32() {
        let mut hasher = Hasher::new();
        hasher.update(stream0_data);
        let computed_crc = hasher.finalize();
        if computed_crc != s0_eof.data_crc32 {
            return Err(ChromeError::CorruptCacheEntry(format!(
                "stream 0 CRC32 mismatch: expected 0x{:08x}, computed 0x{:08x}",
                s0_eof.data_crc32, computed_crc
            )));
        }
    }

    // 3. Parse Stream 1 EOF directly preceding Stream 0 data
    let s1_eof_start = s0_start
        .checked_sub(EOF_RECORD_SIZE)
        .ok_or_else(|| ChromeError::CorruptCacheEntry("stream 1 EOF underflow".into()))?;
    let s1_eof = SimpleFileEOF::parse(&file_bytes[s1_eof_start..s0_start])?;

    let s1_size = s1_eof.stream_size as usize;
    let s1_start = s1_eof_start
        .checked_sub(s1_size)
        .ok_or_else(|| ChromeError::CorruptCacheEntry("stream 1 offset underflow".into()))?;

    if s1_start < HEADER_SIZE {
        return Err(ChromeError::CorruptCacheEntry(
            "stream 1 offset underflow below header".into(),
        ));
    }

    let stream1_data = &file_bytes[s1_start..s1_eof_start];
    if s1_eof.has_crc32() {
        let mut hasher = Hasher::new();
        hasher.update(stream1_data);
        let computed_crc = hasher.finalize();
        if computed_crc != s1_eof.data_crc32 {
            return Err(ChromeError::CorruptCacheEntry(format!(
                "stream 1 CRC32 mismatch: expected 0x{:08x}, computed 0x{:08x}",
                s1_eof.data_crc32, computed_crc
            )));
        }
    }

    // 4. Parse Header at offset 0
    let header = SimpleFileHeader::parse(&file_bytes[0..HEADER_SIZE])?;
    let key_len = header.key_length as usize;

    // Cross-check: stream 1 payload must start immediately after header + key
    if s1_start != HEADER_SIZE + key_len {
        return Err(ChromeError::CorruptCacheEntry(format!(
            "stream 1 start offset {} does not match header + key_length {}",
            s1_start,
            HEADER_SIZE + key_len
        )));
    }

    let key_bytes = &file_bytes[HEADER_SIZE..HEADER_SIZE + key_len];
    let key = String::from_utf8_lossy(key_bytes).to_string();

    Ok(RawCacheEntry {
        key,
        stream0_headers: stream0_data.to_vec(),
        stream1_payload: stream1_data.to_vec(),
    })
}

/// Parses a Chromium Simple Cache entry from a file on disk.
pub fn parse_cache_entry_file(path: &Path) -> Result<RawCacheEntry> {
    let bytes = fs::read(path)?;
    parse_cache_entry_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_profile::FakeChromeProfile;

    #[test]
    fn test_parse_cache_entry_from_fake_profile() {
        let tmp = std::env::temp_dir().join(format!(
            "cache_parse_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp).unwrap();

        let profile = FakeChromeProfile::create(&tmp, "Default").unwrap();
        let key = "https://example.com/images/test.jpg";
        let body = b"test-image-content-bytes-12345";
        let path = profile
            .add_cache_entry(key, body, Some("image/jpeg"), None)
            .unwrap();

        let parsed = parse_cache_entry_file(&path).unwrap();
        assert_eq!(parsed.key, key);
        assert_eq!(parsed.stream1_payload, body);
        assert!(!parsed.stream0_headers.is_empty());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_corrupt_cache_entry_rejected() {
        // Less than 72 bytes
        assert!(parse_cache_entry_bytes(&[0u8; 50]).is_err());

        // Invalid magic numbers
        let mut buf = vec![0u8; 100];
        assert!(parse_cache_entry_bytes(&buf).is_err());

        // Header magic but invalid EOF magic
        buf[0..8].copy_from_slice(&SIMPLE_INITIAL_MAGIC.to_le_bytes());
        assert!(parse_cache_entry_bytes(&buf).is_err());
    }
}
