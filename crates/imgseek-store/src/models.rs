#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corpus {
    pub corpus_id: String,
    pub kind: String,
    pub root_uri: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    pub doc_id: i64,
    pub sha256: [u8; 32],
    pub byte_size: i64,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub ahash: u64,
    pub dhash: u64,
    pub phash: u64,
    pub whash: u64,
    pub chroma_blob_ref: String,
    pub keypoints_ref: Option<String>,
    pub indexed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDoc {
    pub sha256: [u8; 32],
    pub byte_size: i64,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub ahash: u64,
    pub dhash: u64,
    pub phash: u64,
    pub whash: u64,
    pub chroma_blob_ref: String,
    pub keypoints_ref: Option<String>,
    pub indexed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    pub sighting_id: i64,
    pub doc_id: i64,
    pub corpus_id: String,
    pub origin_uri: String,
    pub title: Option<String>,
    pub discovered_at: i64,
    pub file_size: Option<i64>,
    pub file_mtime: Option<i64>,
    pub http_etag: Option<String>,
    pub http_last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSighting {
    pub doc_id: i64,
    pub corpus_id: String,
    pub origin_uri: String,
    pub title: Option<String>,
    pub discovered_at: i64,
    pub file_size: Option<i64>,
    pub file_mtime: Option<i64>,
    pub http_etag: Option<String>,
    pub http_last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRecord {
    pub sha256: [u8; 32],
    pub byte_size: i64,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub ahash: u64,
    pub dhash: u64,
    pub phash: u64,
    pub whash: u64,
    pub chroma_blob_ref: String,
    pub keypoints_ref: Option<String>,
    pub indexed_at: i64,
    pub corpus_id: String,
    pub origin_uri: String,
    pub title: Option<String>,
    pub discovered_at: i64,
    pub file_size: Option<i64>,
    pub file_mtime: Option<i64>,
}

pub type SightingMetaMap = std::collections::HashMap<String, (Option<i64>, Option<i64>)>;
