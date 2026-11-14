pub mod compaction;
pub mod compression;
pub mod error;
pub mod manifest;
pub mod mih;
pub mod occupancy;
pub mod segment;
pub mod verify;

pub use compaction::{compact_segments, mark_tombstone, CompactionResult};
pub use compression::{
    decode_for_block, decode_postings, encode_for_block, encode_postings, read_varint,
    write_varint, FOR_BLOCK_SIZE,
};
pub use error::{IndexError, Result};
pub use manifest::{Manifest, SegmentDescriptor, CURRENT_FORMAT_VERSION, MANIFEST_FILENAME};
pub use mih::{
    ball_masks, extract_chunks_256, extract_chunks_64, generate_ball_masks, get_ball_masks,
    sphere_masks, CandidateHit, HashAlgoId, MihChunkTable, MihChunkTableView, MihEnsembleIndex,
    MihFamilyIndex, BUCKET_COUNT, BUCKET_ENTRY_SIZE, CHUNK_DIRECTORY_SIZE, TOTAL_BALL_MASKS_R3,
};
pub use occupancy::{analyze_index_segment, ChunkTableOccupancy, OccupancyReport};
pub use segment::{
    ChunkTableDescriptor, DataSegmentReader, DataSegmentWriter, DocumentRecord, IndexSegmentReader,
    IndexSegmentWriter, SegmentFooter, SegmentHeader, DATA_MAGIC, FOOTER_LEN, FOOTER_MAGIC,
    HEADER_LEN, IDX_MAGIC, RECORD_LEN,
};
pub use verify::{verify_index, verify_segment, SegmentVerification, VerificationReport};
