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

/// Parsed HTTP header values relevant for cache indexing and timestamps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpHeaderInfo {
    pub content_type: Option<String>,
    pub date_str: Option<String>,
    pub date_timestamp: Option<i64>,
    pub last_modified_str: Option<String>,
    pub last_modified_timestamp: Option<i64>,
}

/// Fully parsed and interpreted Chromium Simple Cache entry.
#[derive(Debug, Clone)]
pub struct ParsedCacheItem {
    pub raw_key: String,
    pub resource_url: String,
    pub top_frame_site: Option<String>,
    pub headers: HttpHeaderInfo,
    pub image_format: Option<String>,
    pub payload: Vec<u8>,
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let mp = (m + if m > 2 { 0 } else { 12 }) as i64;
    let doy = (153 * (mp - 3) + 2) / 5 + d as i64 - 1;
    let doe = yoe as i64 * 365 + yoe as i64 / 4 - yoe as i64 / 100 + doy;
    era * 146097 + doe - 719468
}

fn month_from_str(s: &str) -> Option<u32> {
    match s.to_ascii_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

/// Leniently parses an RFC 1123 / RFC 850 / asctime HTTP date string into Unix epoch seconds.
pub fn parse_http_date(s: &str) -> Option<i64> {
    let clean = s.trim();
    let parts: Vec<&str> = clean
        .split(|c: char| c.is_whitespace() || c == ',' || c == '-')
        .filter(|p| !p.is_empty())
        .collect();

    let mut month_idx = None;
    for (i, part) in parts.iter().enumerate() {
        if month_from_str(part).is_some() {
            month_idx = Some(i);
            break;
        }
    }

    let m_idx = month_idx?;
    let month = month_from_str(parts[m_idx])?;

    let (day, year, time_str) =
        if m_idx > 0 && m_idx + 2 < parts.len() && parts[m_idx + 2].contains(':') {
            let day: u32 = parts[m_idx - 1].parse().ok()?;
            let mut year: i64 = parts[m_idx + 1].parse().ok()?;
            if year < 100 {
                year += if year < 70 { 2000 } else { 1900 };
            }
            (day, year, parts[m_idx + 2])
        } else if m_idx + 3 < parts.len() && parts[m_idx + 2].contains(':') {
            let day: u32 = parts[m_idx + 1].parse().ok()?;
            let time_str = parts[m_idx + 2];
            let year: i64 = parts[m_idx + 3].parse().ok()?;
            (day, year, time_str)
        } else {
            return None;
        };

    let time_parts: Vec<&str> = time_str.split(':').collect();
    if time_parts.len() != 3 {
        return None;
    }
    let hour: u32 = time_parts[0].parse().ok()?;
    let min: u32 = time_parts[1].parse().ok()?;
    let sec: u32 = time_parts[2].parse().ok()?;

    if day == 0 || day > 31 || hour > 23 || min > 59 || sec > 60 {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let total_secs = days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64;
    Some(total_secs)
}

/// Parses a cache key (simple or modern partitioned key) into `(resource_url, top_frame_site)`.
pub fn parse_cache_key(key: &str) -> (String, Option<String>) {
    let tokens: Vec<&str> = key.split_whitespace().collect();
    if tokens.is_empty() {
        return (String::new(), None);
    }

    if tokens.len() == 1 {
        let single = tokens[0];
        let cleaned = if let Some(stripped) = single.strip_prefix("1/0/") {
            stripped
        } else if let Some(stripped) = single.strip_prefix("0/0/") {
            stripped
        } else if let Some(stripped) = single.strip_prefix("2/0/") {
            stripped
        } else {
            single
        };
        (cleaned.to_string(), None)
    } else {
        let resource_url = tokens[tokens.len() - 1].to_string();
        let mut top_frame_site = None;

        for &tok in &tokens[..tokens.len() - 1] {
            if let Some(pos) = tok.find("_dk_") {
                let site = &tok[pos + 4..];
                if !site.is_empty() {
                    top_frame_site = Some(site.trim_end_matches('/').to_string());
                    break;
                }
            } else if tok.starts_with("http://") || tok.starts_with("https://") {
                top_frame_site = Some(tok.trim_end_matches('/').to_string());
                break;
            }
        }

        (resource_url, top_frame_site)
    }
}

/// Leniently extracts headers from HTTP stream 0 bytes (NUL or CRLF delimited).
pub fn parse_http_headers(bytes: &[u8]) -> HttpHeaderInfo {
    let mut info = HttpHeaderInfo::default();
    let text = String::from_utf8_lossy(bytes);

    let raw_lines: Vec<&str> = text
        .split(['\0', '\r', '\n'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for line in raw_lines {
        if let Some((k, v)) = line.split_once(':') {
            let k_lower = k.trim().to_ascii_lowercase();
            let v_trim = v.trim();
            match k_lower.as_str() {
                "content-type" => {
                    info.content_type = Some(v_trim.to_string());
                }
                "date" => {
                    info.date_str = Some(v_trim.to_string());
                    info.date_timestamp = parse_http_date(v_trim);
                }
                "last-modified" => {
                    info.last_modified_str = Some(v_trim.to_string());
                    info.last_modified_timestamp = parse_http_date(v_trim);
                }
                _ => {}
            }
        }
    }

    info
}

/// Converts a raw cache entry into a structured `ParsedCacheItem` with format sniffing.
pub fn parse_cache_item(raw: &RawCacheEntry) -> ParsedCacheItem {
    let (resource_url, top_frame_site) = parse_cache_key(&raw.key);
    let headers = parse_http_headers(&raw.stream0_headers);

    let is_gzip = raw.stream1_payload.len() >= 2
        && raw.stream1_payload[0] == 0x1f
        && raw.stream1_payload[1] == 0x8b;

    let image_format = if is_gzip {
        None
    } else {
        imgseek_core::sniff_format(&raw.stream1_payload)
            .ok()
            .map(|f| format!("{:?}", f).to_lowercase())
    };

    ParsedCacheItem {
        raw_key: raw.key.clone(),
        resource_url,
        top_frame_site,
        headers,
        image_format,
        payload: raw.stream1_payload.clone(),
    }
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
        let key =
            "_dk_https://example.com https://example.com https://cdn.example.com/images/test.bmp";
        // 2x2 BMP
        let body: [u8; 70] = [
            0x42, 0x4d, 70, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0,
            1, 0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 255, 255, 255, 0, 0, 0,
        ];
        let path = profile
            .add_cache_entry(
                key,
                &body,
                Some("image/bmp"),
                Some("Sun, 15 Nov 2026 12:00:00 GMT"),
            )
            .unwrap();

        let raw = parse_cache_entry_file(&path).unwrap();
        assert_eq!(raw.key, key);
        assert_eq!(raw.stream1_payload, body);

        let item = parse_cache_item(&raw);
        assert_eq!(item.resource_url, "https://cdn.example.com/images/test.bmp");
        assert_eq!(item.top_frame_site.as_deref(), Some("https://example.com"));
        assert_eq!(item.headers.content_type.as_deref(), Some("image/bmp"));
        assert!(item.headers.date_timestamp.is_some());
        assert_eq!(item.image_format.as_deref(), Some("bmp"));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_http_date() {
        let ts = parse_http_date("Sun, 15 Nov 2026 12:00:00 GMT").unwrap();
        assert!(ts > 1_700_000_000);

        let ts2 = parse_http_date("Sunday, 06-Nov-94 08:49:37 GMT").unwrap();
        assert_eq!(ts2, 784111777);

        assert!(parse_http_date("invalid date string").is_none());
    }

    #[test]
    fn test_parse_cache_key() {
        let (res1, top1) = parse_cache_key("https://example.com/photo.jpg");
        assert_eq!(res1, "https://example.com/photo.jpg");
        assert_eq!(top1, None);

        let (res2, top2) = parse_cache_key("1/0/https://example.com/photo.jpg");
        assert_eq!(res2, "https://example.com/photo.jpg");
        assert_eq!(top2, None);

        let (res3, top3) = parse_cache_key(
            "1/0/_dk_https://site.org https://site.org https://static.site.org/cat.png",
        );
        assert_eq!(res3, "https://static.site.org/cat.png");
        assert_eq!(top3.as_deref(), Some("https://site.org"));
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
