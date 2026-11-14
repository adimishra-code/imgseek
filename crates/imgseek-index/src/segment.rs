use crate::error::{IndexError, Result};
use crc32fast::Hasher;
use memmap2::Mmap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DATA_MAGIC: &[u8; 8] = b"SEGKDAT\0";
pub const IDX_MAGIC: &[u8; 8] = b"SEGKIDX\0";
pub const FOOTER_MAGIC: &[u8; 4] = b"\xDE\xAD\xBE\xEF";

pub const HEADER_LEN: usize = 32;
pub const FOOTER_LEN: usize = 16;
pub const RECORD_LEN: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentHeader {
    pub magic: [u8; 8],
    pub format_version: u32,
    pub segment_id: u64,
    pub doc_count: u32,
    pub reserved: u64,
}

impl SegmentHeader {
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut buf = [0u8; HEADER_LEN];
        buf[0..8].copy_from_slice(&self.magic);
        buf[8..12].copy_from_slice(&self.format_version.to_le_bytes());
        buf[12..20].copy_from_slice(&self.segment_id.to_le_bytes());
        buf[20..24].copy_from_slice(&self.doc_count.to_le_bytes());
        buf[24..32].copy_from_slice(&self.reserved.to_le_bytes());
        buf
    }

    pub fn from_bytes(bytes: &[u8; HEADER_LEN]) -> Self {
        let mut magic = [0u8; 8];
        magic.copy_from_slice(&bytes[0..8]);
        let format_version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let segment_id = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        let doc_count = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
        let reserved = u64::from_le_bytes(bytes[24..32].try_into().unwrap());

        Self {
            magic,
            format_version,
            segment_id,
            doc_count,
            reserved,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentFooter {
    pub payload_len: u64,
    pub crc32c: u32,
    pub footer_magic: [u8; 4],
}

impl SegmentFooter {
    pub fn to_bytes(&self) -> [u8; FOOTER_LEN] {
        let mut buf = [0u8; FOOTER_LEN];
        buf[0..8].copy_from_slice(&self.payload_len.to_le_bytes());
        buf[8..12].copy_from_slice(&self.crc32c.to_le_bytes());
        buf[12..16].copy_from_slice(&self.footer_magic);
        buf
    }

    pub fn from_bytes(bytes: &[u8; FOOTER_LEN]) -> Self {
        let payload_len = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
        let crc32c = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
        let mut footer_magic = [0u8; 4];
        footer_magic.copy_from_slice(&bytes[12..16]);

        Self {
            payload_len,
            crc32c,
            footer_magic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentRecord {
    pub doc_id: u32,
    pub phash: u64,
    pub whash: u64,
    pub dhash: u64,
    pub ahash: u64,
    pub pdq: [u8; 32],
    pub pdq_quality: u16,
    pub width: u16,
    pub height: u16,
    pub chroma_ref: [u8; 32],
}

impl DocumentRecord {
    pub fn to_bytes(&self) -> [u8; RECORD_LEN] {
        let mut buf = [0u8; RECORD_LEN];
        buf[0..4].copy_from_slice(&self.doc_id.to_le_bytes());
        buf[4..12].copy_from_slice(&self.phash.to_le_bytes());
        buf[12..20].copy_from_slice(&self.whash.to_le_bytes());
        buf[20..28].copy_from_slice(&self.dhash.to_le_bytes());
        buf[28..36].copy_from_slice(&self.ahash.to_le_bytes());
        buf[36..68].copy_from_slice(&self.pdq);
        buf[68..70].copy_from_slice(&self.pdq_quality.to_le_bytes());
        buf[70..72].copy_from_slice(&self.width.to_le_bytes());
        buf[72..74].copy_from_slice(&self.height.to_le_bytes());
        buf[74..106].copy_from_slice(&self.chroma_ref);
        // remaining 106..128 is zero padding
        buf
    }

    pub fn from_bytes(bytes: &[u8; RECORD_LEN]) -> Self {
        let doc_id = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let phash = u64::from_le_bytes(bytes[4..12].try_into().unwrap());
        let whash = u64::from_le_bytes(bytes[12..20].try_into().unwrap());
        let dhash = u64::from_le_bytes(bytes[20..28].try_into().unwrap());
        let ahash = u64::from_le_bytes(bytes[28..36].try_into().unwrap());
        let mut pdq = [0u8; 32];
        pdq.copy_from_slice(&bytes[36..68]);
        let pdq_quality = u16::from_le_bytes(bytes[68..70].try_into().unwrap());
        let width = u16::from_le_bytes(bytes[70..72].try_into().unwrap());
        let height = u16::from_le_bytes(bytes[72..74].try_into().unwrap());
        let mut chroma_ref = [0u8; 32];
        chroma_ref.copy_from_slice(&bytes[74..106]);

        Self {
            doc_id,
            phash,
            whash,
            dhash,
            ahash,
            pdq,
            pdq_quality,
            width,
            height,
            chroma_ref,
        }
    }
}

pub struct DataSegmentWriter {
    segment_id: u64,
    final_path: PathBuf,
    tmp_path: PathBuf,
    file: File,
    hasher: Hasher,
    records_written: u32,
    min_doc_id: Option<u32>,
    max_doc_id: Option<u32>,
}

impl DataSegmentWriter {
    pub fn create<P: AsRef<Path>>(index_dir: P, segment_id: u64) -> Result<Self> {
        let dir = index_dir.as_ref();
        fs::create_dir_all(dir)?;

        let filename = format!("seg_{:08}.data", segment_id);
        let tmp_filename = format!("seg_{:08}.data.tmp", segment_id);

        let final_path = dir.join(filename);
        let tmp_path = dir.join(tmp_filename);

        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp_path)?;
        let mut hasher = Hasher::new();

        // Write initial header placeholder (doc_count will be finalized upon finish)
        let header = SegmentHeader {
            magic: *DATA_MAGIC,
            format_version: 1,
            segment_id,
            doc_count: 0,
            reserved: 0,
        };
        let header_bytes = header.to_bytes();
        file.write_all(&header_bytes)?;
        hasher.update(&header_bytes);

        Ok(Self {
            segment_id,
            final_path,
            tmp_path,
            file,
            hasher,
            records_written: 0,
            min_doc_id: None,
            max_doc_id: None,
        })
    }

    pub fn add_record(&mut self, record: &DocumentRecord) -> Result<()> {
        let bytes = record.to_bytes();
        self.file.write_all(&bytes)?;
        self.hasher.update(&bytes);

        self.records_written += 1;
        self.min_doc_id = Some(
            self.min_doc_id
                .map_or(record.doc_id, |m| m.min(record.doc_id)),
        );
        self.max_doc_id = Some(
            self.max_doc_id
                .map_or(record.doc_id, |m| m.max(record.doc_id)),
        );

        Ok(())
    }

    pub fn finish(mut self) -> Result<(PathBuf, u32, u32, u32)> {
        // Rewrite header at start of file with exact doc_count
        let header = SegmentHeader {
            magic: *DATA_MAGIC,
            format_version: 1,
            segment_id: self.segment_id,
            doc_count: self.records_written,
            reserved: 0,
        };
        let header_bytes = header.to_bytes();

        use std::io::Seek;
        self.file.seek(std::io::SeekFrom::Start(0))?;
        self.file.write_all(&header_bytes)?;

        // Recompute CRC from scratch including updated header
        let payload_len = (self.records_written as usize * RECORD_LEN) as u64;
        self.file.seek(std::io::SeekFrom::Start(0))?;

        let mut final_hasher = Hasher::new();
        final_hasher.update(&header_bytes);

        // Stream and hash payload
        self.file
            .seek(std::io::SeekFrom::Start(HEADER_LEN as u64))?;
        let mut buf = [0u8; 8192];
        let mut remaining = payload_len;
        while remaining > 0 {
            let to_read = (remaining as usize).min(buf.len());
            use std::io::Read;
            self.file.read_exact(&mut buf[..to_read])?;
            final_hasher.update(&buf[..to_read]);
            remaining -= to_read as u64;
        }

        let crc = final_hasher.finalize();

        // Write footer
        let footer = SegmentFooter {
            payload_len,
            crc32c: crc,
            footer_magic: *FOOTER_MAGIC,
        };
        let footer_bytes = footer.to_bytes();
        self.file.write_all(&footer_bytes)?;
        self.file.sync_all()?;
        drop(self.file);

        if self.final_path.exists() {
            let _ = fs::remove_file(&self.final_path);
        }
        fs::rename(&self.tmp_path, &self.final_path)?;

        let min_id = self.min_doc_id.unwrap_or(0);
        let max_id = self.max_doc_id.unwrap_or(0);

        Ok((self.final_path, crc, min_id, max_id))
    }
}

pub struct DataSegmentReader {
    _file: File,
    mmap: Mmap,
    header: SegmentHeader,
    payload_offset: usize,
    doc_count: usize,
}

impl DataSegmentReader {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(&path)?;
        let mmap = unsafe { Mmap::map(&file)? };

        if mmap.len() < HEADER_LEN + FOOTER_LEN {
            return Err(IndexError::CorruptSegment(format!(
                "file {:?} too small: {} bytes",
                path.as_ref(),
                mmap.len()
            )));
        }

        // Validate Header
        let header_bytes: &[u8; HEADER_LEN] = mmap[0..HEADER_LEN].try_into().unwrap();
        let header = SegmentHeader::from_bytes(header_bytes);

        if &header.magic != DATA_MAGIC {
            return Err(IndexError::CorruptSegment(format!(
                "invalid data magic: {:?}",
                header.magic
            )));
        }

        if header.format_version != 1 {
            return Err(IndexError::VersionMismatch {
                expected: 1,
                actual: header.format_version,
            });
        }

        // Validate Footer
        let footer_offset = mmap.len() - FOOTER_LEN;
        let footer_bytes: &[u8; FOOTER_LEN] = mmap[footer_offset..].try_into().unwrap();
        let footer = SegmentFooter::from_bytes(footer_bytes);

        if &footer.footer_magic != FOOTER_MAGIC {
            return Err(IndexError::CorruptSegment(
                "invalid footer magic in data segment".into(),
            ));
        }

        let expected_payload_len = (header.doc_count as usize * RECORD_LEN) as u64;
        if footer.payload_len != expected_payload_len {
            return Err(IndexError::CorruptSegment(format!(
                "footer payload length {} does not match doc_count {}",
                footer.payload_len, header.doc_count
            )));
        }

        // Validate CRC32C over header and payload
        let mut hasher = Hasher::new();
        hasher.update(&mmap[0..footer_offset]);
        let computed_crc = hasher.finalize();

        if computed_crc != footer.crc32c {
            return Err(IndexError::ChecksumMismatch {
                expected: footer.crc32c,
                actual: computed_crc,
            });
        }

        Ok(Self {
            _file: file,
            mmap,
            header,
            payload_offset: HEADER_LEN,
            doc_count: header.doc_count as usize,
        })
    }

    pub fn segment_id(&self) -> u64 {
        self.header.segment_id
    }

    pub fn len(&self) -> usize {
        self.doc_count
    }

    pub fn is_empty(&self) -> bool {
        self.doc_count == 0
    }

    pub fn get(&self, index: usize) -> Option<DocumentRecord> {
        if index >= self.doc_count {
            return None;
        }
        let start = self.payload_offset + index * RECORD_LEN;
        let bytes: &[u8; RECORD_LEN] = self.mmap[start..start + RECORD_LEN].try_into().unwrap();
        Some(DocumentRecord::from_bytes(bytes))
    }

    pub fn iter(&self) -> impl Iterator<Item = DocumentRecord> + '_ {
        (0..self.doc_count).map(move |i| self.get(i).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_segment_writer_and_reader_round_trip() {
        let test_dir = std::env::temp_dir().join(format!(
            "data_seg_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let mut writer = DataSegmentWriter::create(&test_dir, 1).expect("create writer");

        let rec1 = DocumentRecord {
            doc_id: 10,
            phash: 0x1122_3344_5566_7788,
            whash: 0x8877_6655_4433_2211,
            dhash: 0xAABB_CCDD_EEFF_0011,
            ahash: 0x0011_2233_4455_6677,
            pdq: [0x5A; 32],
            pdq_quality: 85,
            width: 1920,
            height: 1080,
            chroma_ref: [0xEF; 32],
        };

        let rec2 = DocumentRecord {
            doc_id: 11,
            phash: 0x1234_5678_9ABC_DEF0,
            whash: 0xFEDC_BA98_7654_3210,
            dhash: 0x0123_4567_89AB_CDEF,
            ahash: 0xF0E1_D2C3_B4A5_9687,
            pdq: [0xA5; 32],
            pdq_quality: 42,
            width: 800,
            height: 600,
            chroma_ref: [0xFE; 32],
        };

        writer.add_record(&rec1).expect("add rec1");
        writer.add_record(&rec2).expect("add rec2");

        let (final_path, crc, min_id, max_id) = writer.finish().expect("finish writer");
        assert_eq!(min_id, 10);
        assert_eq!(max_id, 11);
        assert_ne!(crc, 0);

        let reader = DataSegmentReader::open(&final_path).expect("open reader");
        assert_eq!(reader.len(), 2);
        assert_eq!(reader.segment_id(), 1);

        assert_eq!(reader.get(0), Some(rec1));
        assert_eq!(reader.get(1), Some(rec2));
        assert_eq!(reader.get(2), None);

        let collected: Vec<_> = reader.iter().collect();
        assert_eq!(collected, vec![rec1, rec2]);

        let _ = fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_corrupt_data_segment_rejected() {
        let test_dir = std::env::temp_dir().join(format!(
            "data_seg_corrupt_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let mut writer = DataSegmentWriter::create(&test_dir, 2).expect("create writer");
        let rec = DocumentRecord {
            doc_id: 1,
            phash: 1,
            whash: 2,
            dhash: 3,
            ahash: 4,
            pdq: [0; 32],
            pdq_quality: 90,
            width: 100,
            height: 100,
            chroma_ref: [0; 32],
        };
        writer.add_record(&rec).expect("add");
        let (final_path, _, _, _) = writer.finish().expect("finish");

        // Corrupt 1 byte in the payload
        let mut bytes = fs::read(&final_path).expect("read");
        bytes[HEADER_LEN + 5] ^= 0xFF;
        fs::write(&final_path, &bytes).expect("write corrupt");

        let result = DataSegmentReader::open(&final_path);
        assert!(matches!(result, Err(IndexError::ChecksumMismatch { .. })));

        let _ = fs::remove_dir_all(&test_dir);
    }
}
