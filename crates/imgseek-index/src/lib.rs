pub mod compression;
pub mod error;

pub use compression::{
    decode_for_block, decode_postings, encode_for_block, encode_postings, read_varint,
    write_varint, FOR_BLOCK_SIZE,
};
pub use error::{IndexError, Result};
