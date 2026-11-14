use crate::error::{IndexError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MANIFEST_FILENAME: &str = "manifest.json";
pub const MANIFEST_TMP_FILENAME: &str = "manifest.json.tmp";
pub const CURRENT_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentDescriptor {
    pub segment_id: u64,
    pub doc_count: u32,
    pub tombstone_count: u32,
    pub idx_file: String,
    pub data_file: String,
    pub tomb_file: String,
    pub idx_checksum: u32,
    pub data_checksum: u32,
    pub min_doc_id: u32,
    pub max_doc_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format_version: u32,
    pub generation: u64,
    pub created_at_utc: String,
    pub algo_versions: BTreeMap<String, u32>,
    pub segments: Vec<SegmentDescriptor>,
}

impl Default for Manifest {
    fn default() -> Self {
        let mut algo_versions = BTreeMap::new();
        algo_versions.insert("ahash".into(), 1);
        algo_versions.insert("chroma".into(), 1);
        algo_versions.insert("dhash".into(), 1);
        algo_versions.insert("orb".into(), 1);
        algo_versions.insert("pdq".into(), 1);
        algo_versions.insert("phash".into(), 1);
        algo_versions.insert("whash".into(), 1);

        Self {
            format_version: CURRENT_FORMAT_VERSION,
            generation: 1,
            created_at_utc: "2026-11-14T00:00:00Z".into(),
            algo_versions,
            segments: Vec::new(),
        }
    }
}

impl Manifest {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn manifest_path<P: AsRef<Path>>(index_dir: P) -> PathBuf {
        index_dir.as_ref().join(MANIFEST_FILENAME)
    }

    pub fn load<P: AsRef<Path>>(index_dir: P) -> Result<Self> {
        let path = Self::manifest_path(index_dir);
        if !path.exists() {
            return Err(IndexError::CorruptSegment(format!(
                "manifest not found at {:?}",
                path
            )));
        }

        let content = fs::read_to_string(&path)?;
        let manifest: Self = serde_json::from_str(&content).map_err(|e| {
            IndexError::CorruptSegment(format!("failed to parse manifest at {:?}: {}", path, e))
        })?;

        if manifest.format_version != CURRENT_FORMAT_VERSION {
            return Err(IndexError::VersionMismatch {
                expected: CURRENT_FORMAT_VERSION,
                actual: manifest.format_version,
            });
        }

        Ok(manifest)
    }

    pub fn load_or_create<P: AsRef<Path>>(index_dir: P) -> Result<Self> {
        let path = Self::manifest_path(&index_dir);
        if path.exists() {
            Self::load(index_dir)
        } else {
            let manifest = Self::new();
            manifest.save_atomic(index_dir)?;
            Ok(manifest)
        }
    }

    pub fn save_atomic<P: AsRef<Path>>(&self, index_dir: P) -> Result<()> {
        let dir = index_dir.as_ref();
        fs::create_dir_all(dir)?;

        let tmp_path = dir.join(MANIFEST_TMP_FILENAME);
        let final_path = dir.join(MANIFEST_FILENAME);

        let json = serde_json::to_string_pretty(self).map_err(|e| {
            IndexError::CorruptSegment(format!("failed to serialize manifest: {}", e))
        })?;

        let mut file = File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);

        fs::rename(&tmp_path, &final_path)?;

        Ok(())
    }

    pub fn next_segment_id(&self) -> u64 {
        self.segments
            .iter()
            .map(|s| s.segment_id)
            .max()
            .map(|max_id| max_id + 1)
            .unwrap_or(1)
    }

    pub fn add_segment(&mut self, descriptor: SegmentDescriptor) {
        self.generation += 1;
        self.segments.push(descriptor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_serialization_and_atomic_swap() {
        let test_dir = std::env::temp_dir().join(format!(
            "manifest_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let mut manifest = Manifest::new();
        manifest.add_segment(SegmentDescriptor {
            segment_id: 1,
            doc_count: 500,
            tombstone_count: 0,
            idx_file: "seg_00000001.idx".into(),
            data_file: "seg_00000001.data".into(),
            tomb_file: "seg_00000001.tomb".into(),
            idx_checksum: 12345,
            data_checksum: 67890,
            min_doc_id: 1,
            max_doc_id: 500,
        });

        manifest.save_atomic(&test_dir).expect("save atomic");

        let loaded = Manifest::load(&test_dir).expect("load manifest");
        assert_eq!(manifest, loaded);
        assert_eq!(loaded.segments.len(), 1);
        assert_eq!(loaded.next_segment_id(), 2);

        let _ = fs::remove_dir_all(&test_dir);
    }
}
