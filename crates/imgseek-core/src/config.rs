use crate::error::{CoreError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub engine: EngineConfig,
    #[serde(default)]
    pub normalize: NormalizeConfig,
    #[serde(default)]
    pub index: IndexConfig,
    #[serde(default)]
    pub ingest: IngestConfig,
    #[serde(default)]
    pub crawler: CrawlerConfig,
    #[serde(default)]
    pub privacy: PrivacyConfig,
    #[serde(default)]
    pub web: WebConfig,
}

impl Config {
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| CoreError::ConfigError(format!("failed to parse TOML: {e}")))?;
        config.validate()?;
        Ok(config)
    }

    pub fn load_or_default(path: Option<&Path>) -> Result<Self> {
        if let Some(p) = path {
            if p.exists() {
                return Self::load_from_file(p);
            }
        } else {
            let default_path = PathBuf::from("config.toml");
            if default_path.exists() {
                return Self::load_from_file(&default_path);
            }
        }
        let config = Self::default();
        config.validate()?;
        Ok(config)
    }

    pub fn save_default(path: &Path) -> Result<()> {
        let config = Self::default();
        let content = toml::to_string_pretty(&config)
            .map_err(|e| CoreError::ConfigError(format!("failed to serialize config: {e}")))?;
        std::fs::write(path, content)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        let addr = &self.privacy.bind_address;
        if addr != "127.0.0.1" && addr != "localhost" && addr != "::1" && !addr.starts_with("127.")
        {
            return Err(CoreError::ConfigError(format!(
                "privacy.bind_address must be a loopback address, got '{addr}'"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub mode: String,
    pub seed: u64,
    pub chunk_layout: String,
    pub max_candidates: usize,
    pub min_exact_score: f64,
    pub orb_max_features: usize,
    pub orb_fast_threshold: u8,
    pub orb_pattern: String,
    pub pyramid_levels: usize,
    pub pyramid_scale: f64,
    pub vocab_branching: usize,
    pub vocab_depth: usize,
    pub tf_transform: String,
    pub score_mode: String,
    pub max_df_ratio: f64,
    pub rerank_top_k: usize,
    pub ratio_test: f64,
    pub ransac_max_iters: usize,
    pub ransac_inlier_px: f64,
    pub min_inliers: usize,
    pub min_inlier_ratio: f64,
    pub similar_scale: f64,
    #[serde(default)]
    pub tau: TauConfig,
    #[serde(default)]
    pub weights: WeightsConfig,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            mode: "all".to_string(),
            seed: 1337,
            chunk_layout: "contiguous".to_string(),
            max_candidates: 250_000,
            min_exact_score: 0.55,
            orb_max_features: 1500,
            orb_fast_threshold: 20,
            orb_pattern: "gaussian".to_string(),
            pyramid_levels: 8,
            pyramid_scale: 1.2,
            vocab_branching: 10,
            vocab_depth: 4,
            tf_transform: "sqrt".to_string(),
            score_mode: "tfidf".to_string(),
            max_df_ratio: 0.10,
            rerank_top_k: 500,
            ratio_test: 0.8,
            ransac_max_iters: 500,
            ransac_inlier_px: 3.0,
            min_inliers: 15,
            min_inlier_ratio: 0.25,
            similar_scale: 25.0,
            tau: TauConfig::default(),
            weights: WeightsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TauConfig {
    pub phash: u32,
    pub whash: u32,
    pub dhash: u32,
    pub ahash: u32,
    pub pdq: u32,
}

impl Default for TauConfig {
    fn default() -> Self {
        Self {
            phash: 12,
            whash: 10,
            dhash: 10,
            ahash: 12,
            pdq: 31,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightsConfig {
    pub phash: f64,
    pub whash: f64,
    pub dhash: f64,
    pub ahash: f64,
    pub pdq: f64,
    pub chroma: f64,
}

impl Default for WeightsConfig {
    fn default() -> Self {
        Self {
            phash: 0.30,
            whash: 0.25,
            dhash: 0.20,
            ahash: 0.05,
            pdq: 0.20,
            chroma: 0.15,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizeConfig {
    pub max_pixels: u64,
    pub analysis_max_side: u32,
}

impl Default for NormalizeConfig {
    fn default() -> Self {
        Self {
            max_pixels: 40_000_000,
            analysis_max_side: 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    pub dir: PathBuf,
    pub blob_dir: PathBuf,
    pub segment_merge_threshold: usize,
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            dir: PathBuf::from(".imgseek/index"),
            blob_dir: PathBuf::from(".imgseek/blobs"),
            segment_merge_threshold: 8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngestConfig {
    pub blob_mode_folder: String,
    pub blob_mode_crawl: String,
    pub min_dimension: u32,
}

impl Default for IngestConfig {
    fn default() -> Self {
        Self {
            blob_mode_folder: "original".to_string(),
            blob_mode_crawl: "working_copy".to_string(),
            min_dimension: 64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlerConfig {
    pub user_agent: String,
    pub rate_per_domain: f64,
    pub global_concurrency: usize,
    pub max_depth: usize,
    pub max_pages: usize,
    pub max_download_bytes: u64,
    pub blocked_domains: Vec<String>,
}

impl Default for CrawlerConfig {
    fn default() -> Self {
        Self {
            user_agent: "imgseek/0.1 (+https://example.com/CHANGE-ME)".to_string(),
            rate_per_domain: 1.0,
            global_concurrency: 16,
            max_depth: 2,
            max_pages: 500,
            max_download_bytes: 25_000_000,
            blocked_domains: vec![
                "localhost".to_string(),
                "*.internal".to_string(),
                "*.local".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub bind_address: String,
    pub store_browsing_history: bool,
    pub retain_blobs_days: u32,
    pub exclude_patterns: Vec<String>,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            store_browsing_history: true,
            retain_blobs_days: 90,
            exclude_patterns: vec!["*private*".to_string(), "*secret*".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    pub port: u16,
    pub max_upload_bytes: usize,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            max_upload_bytes: 25_000_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_validates() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_serialization_round_trip() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: Config = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(config.engine.seed, parsed.engine.seed);
        assert_eq!(config.normalize.max_pixels, parsed.normalize.max_pixels);
    }

    #[test]
    fn test_invalid_bind_address_rejected() {
        let mut config = Config::default();
        config.privacy.bind_address = "0.0.0.0".to_string();
        assert!(config.validate().is_err());
    }
}
