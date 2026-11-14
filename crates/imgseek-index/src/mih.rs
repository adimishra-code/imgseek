use crate::compression::{decode_postings, encode_postings};
use crate::error::{IndexError, Result};
use crate::segment::DocumentRecord;
use std::io::Write;
use std::sync::OnceLock;

/// Number of buckets in each 16-bit chunk table ($2^{16} = 65,536$).
pub const BUCKET_COUNT: usize = 65536;

/// Directory entry size in bytes: 16-bit key -> (u32 offset, u32 len) = 8 bytes.
pub const BUCKET_ENTRY_SIZE: usize = 8;

/// Bucket directory size in bytes for a chunk table (65,536 * 8 = 524,288 bytes = 512 KB).
pub const CHUNK_DIRECTORY_SIZE: usize = BUCKET_COUNT * BUCKET_ENTRY_SIZE;

/// Total number of ball masks for radii 0, 1, 2, and 3:
/// 1 (radius 0) + 16 (radius 1) + 120 (radius 2) + 560 (radius 3) = 697.
pub const TOTAL_BALL_MASKS_R3: usize = 697;

/// Generate all 16-bit Hamming ball masks for radii 0 through 3 in order.
pub fn generate_ball_masks() -> [u16; TOTAL_BALL_MASKS_R3] {
    let mut masks = [0u16; TOTAL_BALL_MASKS_R3];
    let mut idx = 0;

    // Radius 0: 1 mask (identity)
    masks[idx] = 0;
    idx += 1;

    // Radius 1: 16 masks (single bit set)
    for i in 0..16 {
        masks[idx] = 1 << i;
        idx += 1;
    }

    // Radius 2: 120 masks (pairs of bits set)
    for i in 0..16 {
        for j in (i + 1)..16 {
            masks[idx] = (1 << i) | (1 << j);
            idx += 1;
        }
    }

    // Radius 3: 560 masks (triplets of bits set)
    for i in 0..16 {
        for j in (i + 1)..16 {
            for k in (j + 1)..16 {
                masks[idx] = (1 << i) | (1 << j) | (1 << k);
                idx += 1;
            }
        }
    }

    assert_eq!(idx, TOTAL_BALL_MASKS_R3);
    masks
}

static BALL_MASKS: OnceLock<[u16; TOTAL_BALL_MASKS_R3]> = OnceLock::new();

/// Return a static reference to the precomputed ball masks of radii 0 through 3.
#[inline]
pub fn get_ball_masks() -> &'static [u16; TOTAL_BALL_MASKS_R3] {
    BALL_MASKS.get_or_init(generate_ball_masks)
}

/// Return the Hamming sphere masks with exact Hamming weight equal to `radius`.
///
/// - `radius = 0`: 1 mask (weight 0)
/// - `radius = 1`: 16 masks (weight 1)
/// - `radius = 2`: 120 masks (weight 2)
/// - `radius = 3`: 560 masks (weight 3)
/// - `radius > 3`: empty slice
#[inline]
pub fn sphere_masks(radius: u8) -> &'static [u16] {
    let all = get_ball_masks();
    match radius {
        0 => &all[0..1],
        1 => &all[1..17],
        2 => &all[17..137],
        3 => &all[137..697],
        _ => &[],
    }
}

/// Return the Hamming ball masks with Hamming weight less than or equal to `max_radius`.
///
/// - `max_radius = 0`: 1 mask (weight <= 0)
/// - `max_radius = 1`: 17 masks (weight <= 1)
/// - `max_radius = 2`: 137 masks (weight <= 2)
/// - `max_radius = 3`: 697 masks (weight <= 3)
/// - `max_radius > 3`: all 697 masks
#[inline]
pub fn ball_masks(max_radius: u8) -> &'static [u16] {
    let all = get_ball_masks();
    match max_radius {
        0 => &all[0..1],
        1 => &all[0..17],
        2 => &all[0..137],
        _ => &all[0..697],
    }
}

/// Extract 4 16-bit chunks from a 64-bit hash in little-endian order.
#[inline]
pub fn extract_chunks_64(val: u64) -> [u16; 4] {
    [
        (val & 0xFFFF) as u16,
        ((val >> 16) & 0xFFFF) as u16,
        ((val >> 32) & 0xFFFF) as u16,
        ((val >> 48) & 0xFFFF) as u16,
    ]
}

/// Extract 16 16-bit chunks from a 256-bit hash (such as PDQ) in little-endian order.
#[inline]
pub fn extract_chunks_256(val: &[u8; 32]) -> [u16; 16] {
    let mut chunks = [0u16; 16];
    for i in 0..16 {
        chunks[i] = u16::from_le_bytes([val[2 * i], val[2 * i + 1]]);
    }
    chunks
}

/// Identifier for hash algorithms indexed in Multi-Index Hash tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HashAlgoId {
    PHash = 0,
    WHash = 1,
    DHash = 2,
    AHash = 3,
    Pdq = 4,
}

impl HashAlgoId {
    pub fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::PHash),
            1 => Some(Self::WHash),
            2 => Some(Self::DHash),
            3 => Some(Self::AHash),
            4 => Some(Self::Pdq),
            _ => None,
        }
    }
}

/// In-memory builder representation of a single 16-bit chunk table.
/// Contains 65,536 buckets mapping 16-bit chunk keys to lists of document IDs.
#[derive(Debug, Clone)]
pub struct MihChunkTable {
    buckets: Vec<Vec<u32>>,
    total_docs: usize,
}

impl Default for MihChunkTable {
    fn default() -> Self {
        Self::new()
    }
}

impl MihChunkTable {
    /// Create a new chunk table with 65,536 empty buckets.
    pub fn new() -> Self {
        Self {
            buckets: vec![Vec::new(); BUCKET_COUNT],
            total_docs: 0,
        }
    }

    /// Insert a document ID into the specified 16-bit chunk key bucket.
    pub fn insert(&mut self, key: u16, doc_id: u32) {
        self.buckets[key as usize].push(doc_id);
        self.total_docs += 1;
    }

    /// Return the raw slice of document IDs for an exact 16-bit chunk key.
    pub fn get(&self, key: u16) -> &[u32] {
        &self.buckets[key as usize]
    }

    /// Total number of doc ID insertions in this table.
    pub fn doc_count(&self) -> usize {
        self.total_docs
    }

    /// Number of document IDs in a given bucket.
    pub fn bucket_len(&self, key: u16) -> usize {
        self.buckets[key as usize].len()
    }

    /// Sort and deduplicate all bucket document lists in-place.
    pub fn sort_and_dedup(&mut self) {
        let mut count = 0;
        for bucket in &mut self.buckets {
            bucket.sort_unstable();
            bucket.dedup();
            count += bucket.len();
        }
        self.total_docs = count;
    }

    /// Number of buckets with at least one document ID.
    pub fn non_empty_buckets(&self) -> usize {
        self.buckets.iter().filter(|b| !b.is_empty()).count()
    }

    /// Serialize this chunk table to a writer.
    ///
    /// Layout:
    /// - 524,288 bytes (65,536 * 8 bytes): Directory of (u32 relative_postings_offset, u32 compressed_byte_len).
    /// - Variable-length compressed postings stream (FOR + varint).
    ///
    /// Returns the total bytes written.
    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<u64> {
        let mut directory = vec![0u8; CHUNK_DIRECTORY_SIZE];
        let mut postings_stream = Vec::new();

        for (bucket_idx, doc_ids) in self.buckets.iter().enumerate() {
            if doc_ids.is_empty() {
                continue;
            }

            let mut sorted = doc_ids.clone();
            sorted.sort_unstable();
            sorted.dedup();

            let offset = postings_stream.len() as u32;
            encode_postings(&sorted, &mut postings_stream)?;
            let len = (postings_stream.len() as u32) - offset;

            let dir_offset = bucket_idx * BUCKET_ENTRY_SIZE;
            directory[dir_offset..dir_offset + 4].copy_from_slice(&offset.to_le_bytes());
            directory[dir_offset + 4..dir_offset + 8].copy_from_slice(&len.to_le_bytes());
        }

        writer.write_all(&directory)?;
        writer.write_all(&postings_stream)?;

        Ok((directory.len() + postings_stream.len()) as u64)
    }
}

/// Zero-copy read-only view of a serialized chunk table backed by memory-mapped bytes.
#[derive(Debug, Clone, Copy)]
pub struct MihChunkTableView<'a> {
    directory: &'a [u8],
    postings_stream: &'a [u8],
}

impl<'a> MihChunkTableView<'a> {
    /// Construct a view from a byte slice containing the directory and postings stream.
    pub fn from_bytes(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < CHUNK_DIRECTORY_SIZE {
            return Err(IndexError::CorruptSegment(format!(
                "chunk table slice length {} is smaller than directory size {}",
                bytes.len(),
                CHUNK_DIRECTORY_SIZE
            )));
        }

        let (directory, postings_stream) = bytes.split_at(CHUNK_DIRECTORY_SIZE);
        Ok(Self {
            directory,
            postings_stream,
        })
    }

    /// Check whether a 16-bit chunk key bucket has no postings.
    #[inline]
    pub fn is_empty(&self, key: u16) -> bool {
        let dir_offset = (key as usize) * BUCKET_ENTRY_SIZE;
        let len = u32::from_le_bytes(
            self.directory[dir_offset + 4..dir_offset + 8]
                .try_into()
                .unwrap(),
        );
        len == 0
    }

    /// Return the compressed byte length of a given 16-bit bucket.
    #[inline]
    pub fn bucket_byte_len(&self, key: u16) -> u32 {
        let dir_offset = (key as usize) * BUCKET_ENTRY_SIZE;
        u32::from_le_bytes(
            self.directory[dir_offset + 4..dir_offset + 8]
                .try_into()
                .unwrap(),
        )
    }

    /// Retrieve and decompress document IDs for an exact 16-bit key.
    pub fn get(&self, key: u16) -> Result<Vec<u32>> {
        let dir_offset = (key as usize) * BUCKET_ENTRY_SIZE;
        let offset = u32::from_le_bytes(
            self.directory[dir_offset..dir_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        let len = u32::from_le_bytes(
            self.directory[dir_offset + 4..dir_offset + 8]
                .try_into()
                .unwrap(),
        ) as usize;

        if len == 0 {
            return Ok(Vec::new());
        }

        if offset + len > self.postings_stream.len() {
            return Err(IndexError::CorruptSegment(format!(
                "bucket offset {} len {} out of bounds (postings len {})",
                offset,
                len,
                self.postings_stream.len()
            )));
        }

        let compressed = &self.postings_stream[offset..offset + len];
        let mut cursor = 0;
        let mut doc_ids = Vec::new();
        decode_postings(compressed, &mut cursor, &mut doc_ids)?;
        Ok(doc_ids)
    }
}

/// An index family representing all $m$ chunk tables for a specific hash algorithm.
/// For 64-bit hashes, $m = 4$ chunk tables of 16 bits each.
/// For 256-bit hashes (PDQ), $m = 16$ chunk tables of 16 bits each.
#[derive(Debug, Clone)]
pub struct MihFamilyIndex {
    pub algo_id: HashAlgoId,
    pub chunk_tables: Vec<MihChunkTable>,
}

impl MihFamilyIndex {
    /// Create a 4-chunk family index for a 64-bit hash algorithm.
    pub fn new_64(algo_id: HashAlgoId) -> Self {
        let mut chunk_tables = Vec::with_capacity(4);
        for _ in 0..4 {
            chunk_tables.push(MihChunkTable::new());
        }
        Self {
            algo_id,
            chunk_tables,
        }
    }

    /// Create a 16-chunk family index for a 256-bit hash algorithm (PDQ).
    pub fn new_256(algo_id: HashAlgoId) -> Self {
        let mut chunk_tables = Vec::with_capacity(16);
        for _ in 0..16 {
            chunk_tables.push(MihChunkTable::new());
        }
        Self {
            algo_id,
            chunk_tables,
        }
    }

    /// Insert a 64-bit hash and document ID across all 4 chunk tables.
    pub fn insert_64(&mut self, hash: u64, doc_id: u32) {
        assert_eq!(self.chunk_tables.len(), 4);
        let chunks = extract_chunks_64(hash);
        for (i, &chunk) in chunks.iter().enumerate() {
            self.chunk_tables[i].insert(chunk, doc_id);
        }
    }

    /// Insert a 256-bit hash and document ID across all 16 chunk tables.
    pub fn insert_256(&mut self, hash: &[u8; 32], doc_id: u32) {
        assert_eq!(self.chunk_tables.len(), 16);
        let chunks = extract_chunks_256(hash);
        for (i, &chunk) in chunks.iter().enumerate() {
            self.chunk_tables[i].insert(chunk, doc_id);
        }
    }

    /// Number of chunk tables in this family.
    pub fn chunk_count(&self) -> usize {
        self.chunk_tables.len()
    }

    /// Sort and deduplicate all chunk tables in this family.
    pub fn sort_and_dedup(&mut self) {
        for table in &mut self.chunk_tables {
            table.sort_and_dedup();
        }
    }
}

/// Ensemble of all hash families for a segment:
/// - pHash (64-bit, 4 chunks)
/// - wHash (64-bit, 4 chunks)
/// - dHash (64-bit, 4 chunks)
/// - PDQ (256-bit, 16 chunks)
///
/// Total chunk tables = 4 + 4 + 4 + 16 = 28 chunk tables.
#[derive(Debug, Clone)]
pub struct MihEnsembleIndex {
    pub phash: MihFamilyIndex,
    pub whash: MihFamilyIndex,
    pub dhash: MihFamilyIndex,
    pub pdq: MihFamilyIndex,
    pub doc_count: u32,
}

impl Default for MihEnsembleIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl MihEnsembleIndex {
    /// Create a new ensemble index with empty families for pHash, wHash, dHash, and PDQ.
    pub fn new() -> Self {
        Self {
            phash: MihFamilyIndex::new_64(HashAlgoId::PHash),
            whash: MihFamilyIndex::new_64(HashAlgoId::WHash),
            dhash: MihFamilyIndex::new_64(HashAlgoId::DHash),
            pdq: MihFamilyIndex::new_256(HashAlgoId::Pdq),
            doc_count: 0,
        }
    }

    /// Insert a document record into all hash families.
    pub fn insert_record(&mut self, record: &DocumentRecord) {
        self.phash.insert_64(record.phash, record.doc_id);
        self.whash.insert_64(record.whash, record.doc_id);
        self.dhash.insert_64(record.dhash, record.doc_id);
        self.pdq.insert_256(&record.pdq, record.doc_id);
        self.doc_count += 1;
    }

    /// Sort and deduplicate all buckets across all families.
    pub fn sort_and_dedup(&mut self) {
        self.phash.sort_and_dedup();
        self.whash.sort_and_dedup();
        self.dhash.sort_and_dedup();
        self.pdq.sort_and_dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_ball_masks_counts_and_popcounts() {
        let masks = get_ball_masks();
        assert_eq!(masks.len(), TOTAL_BALL_MASKS_R3);

        // Radius 0: exactly 1 mask with popcount 0
        let r0 = sphere_masks(0);
        assert_eq!(r0.len(), 1);
        assert_eq!(r0[0], 0);
        assert_eq!(r0[0].count_ones(), 0);

        // Radius 1: exactly 16 masks with popcount 1
        let r1 = sphere_masks(1);
        assert_eq!(r1.len(), 16);
        for &m in r1 {
            assert_eq!(m.count_ones(), 1);
        }

        // Radius 2: exactly 120 masks with popcount 2
        let r2 = sphere_masks(2);
        assert_eq!(r2.len(), 120);
        for &m in r2 {
            assert_eq!(m.count_ones(), 2);
        }

        // Radius 3: exactly 560 masks with popcount 3
        let r3 = sphere_masks(3);
        assert_eq!(r3.len(), 560);
        for &m in r3 {
            assert_eq!(m.count_ones(), 3);
        }

        // Verify all 697 masks are unique
        let mut set = HashSet::new();
        for &m in masks.iter() {
            assert!(set.insert(m), "duplicate mask found: 0x{:04X}", m);
        }
        assert_eq!(set.len(), 697);

        // Check cumulative ball masks
        assert_eq!(ball_masks(0).len(), 1);
        assert_eq!(ball_masks(1).len(), 17);
        assert_eq!(ball_masks(2).len(), 137);
        assert_eq!(ball_masks(3).len(), 697);
    }

    #[test]
    fn test_extract_chunks_64() {
        let val: u64 = 0x0123_4567_89AB_CDEF;
        let chunks = extract_chunks_64(val);
        assert_eq!(chunks[0], 0xCDEF);
        assert_eq!(chunks[1], 0x89AB);
        assert_eq!(chunks[2], 0x4567);
        assert_eq!(chunks[3], 0x0123);

        let reconstructed = (chunks[0] as u64)
            | ((chunks[1] as u64) << 16)
            | ((chunks[2] as u64) << 32)
            | ((chunks[3] as u64) << 48);
        assert_eq!(reconstructed, val);

        let zero_chunks = extract_chunks_64(0);
        assert_eq!(zero_chunks, [0, 0, 0, 0]);

        let max_chunks = extract_chunks_64(u64::MAX);
        assert_eq!(max_chunks, [u16::MAX, u16::MAX, u16::MAX, u16::MAX]);
    }

    #[test]
    fn test_extract_chunks_256() {
        let mut bytes = [0u8; 32];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i * 7 + 1) as u8;
        }

        let chunks = extract_chunks_256(&bytes);
        assert_eq!(chunks.len(), 16);

        let mut reconstructed = [0u8; 32];
        for i in 0..16 {
            let chunk_bytes = chunks[i].to_le_bytes();
            reconstructed[2 * i] = chunk_bytes[0];
            reconstructed[2 * i + 1] = chunk_bytes[1];
        }
        assert_eq!(reconstructed, bytes);
    }

    #[test]
    fn test_mih_chunk_table_in_memory() {
        let mut table = MihChunkTable::new();
        assert_eq!(table.doc_count(), 0);
        assert_eq!(table.non_empty_buckets(), 0);

        table.insert(0x1234, 10);
        table.insert(0x1234, 25);
        table.insert(0xABCD, 99);

        assert_eq!(table.doc_count(), 3);
        assert_eq!(table.non_empty_buckets(), 2);
        assert_eq!(table.get(0x1234), &[10, 25]);
        assert_eq!(table.get(0xABCD), &[99]);
        assert_eq!(table.get(0x0000), &[] as &[u32]);
    }

    #[test]
    fn test_mih_chunk_table_serialization_and_view() {
        let mut table = MihChunkTable::new();

        // Bucket 0: small list
        table.insert(0, 10);
        table.insert(0, 50);

        // Bucket 42: large list (> 128 elements to exercise FOR encoding)
        for i in 1..=200 {
            table.insert(42, i * 3);
        }

        // Bucket 65535: single element
        table.insert(65535, 1_000_000);

        let mut buf = Vec::new();
        let bytes_written = table.serialize(&mut buf).unwrap();
        assert_eq!(bytes_written as usize, buf.len());
        assert!(buf.len() > CHUNK_DIRECTORY_SIZE);

        let view = MihChunkTableView::from_bytes(&buf).unwrap();

        // Check bucket 0
        assert!(!view.is_empty(0));
        let docs0 = view.get(0).unwrap();
        assert_eq!(docs0, vec![10, 50]);

        // Check bucket 42
        assert!(!view.is_empty(42));
        let docs42 = view.get(42).unwrap();
        let expected42: Vec<u32> = (1..=200).map(|i| i * 3).collect();
        assert_eq!(docs42, expected42);

        // Check bucket 65535
        assert!(!view.is_empty(65535));
        let docs_max = view.get(65535).unwrap();
        assert_eq!(docs_max, vec![1_000_000]);

        // Check empty bucket
        assert!(view.is_empty(1));
        assert_eq!(view.get(1).unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn test_mih_ensemble_record_indexing() {
        let mut ensemble = MihEnsembleIndex::new();
        assert_eq!(ensemble.phash.chunk_count(), 4);
        assert_eq!(ensemble.whash.chunk_count(), 4);
        assert_eq!(ensemble.dhash.chunk_count(), 4);
        assert_eq!(ensemble.pdq.chunk_count(), 16);

        let rec = DocumentRecord {
            doc_id: 1,
            phash: 0x1111_2222_3333_4444,
            whash: 0x5555_6666_7777_8888,
            dhash: 0x9999_AAAA_BBBB_CCCC,
            ahash: 0,
            pdq: [0x42; 32],
            pdq_quality: 90,
            width: 800,
            height: 600,
            chroma_ref: [0; 32],
        };

        ensemble.insert_record(&rec);
        assert_eq!(ensemble.doc_count, 1);

        // Check phash chunks
        let phash_chunks = extract_chunks_64(rec.phash);
        for (i, &chunk) in phash_chunks.iter().enumerate() {
            assert_eq!(ensemble.phash.chunk_tables[i].get(chunk), &[1]);
        }

        // Check pdq chunks
        let pdq_chunks = extract_chunks_256(&rec.pdq);
        for (i, &chunk) in pdq_chunks.iter().enumerate() {
            assert_eq!(ensemble.pdq.chunk_tables[i].get(chunk), &[1]);
        }
    }
}
