pub mod compression;
pub mod error;
pub mod manifest;
pub mod segment;

pub use compression::{
    decode_for_block, decode_postings, encode_for_block, encode_postings, read_varint,
    write_varint, FOR_BLOCK_SIZE,
};
pub use error::{IndexError, Result};
pub use manifest::{Manifest, SegmentDescriptor, CURRENT_FORMAT_VERSION, MANIFEST_FILENAME};
pub use segment::{
    DataSegmentReader, DataSegmentWriter, DocumentRecord, SegmentFooter, SegmentHeader, DATA_MAGIC,
    FOOTER_LEN, FOOTER_MAGIC, HEADER_LEN, IDX_MAGIC, RECORD_LEN,
};
