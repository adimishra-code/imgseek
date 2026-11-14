use std::path::Path;

pub use imgseek_core::ScoreBreakdown;
use imgseek_core::{
    ahash_from_luma, bytes_to_chroma, compute_color_histogram, compute_combined_score,
    dhash_from_luma, normalize, pdq_from_luma, phash_from_luma, whash_from_luma, Config,
};
use imgseek_store::{bytes_to_hex, BlobStore, Database, Doc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgoVersions {
    pub phash: u32,
    pub whash: u32,
    pub dhash: u32,
    pub ahash: u32,
    pub pdq: u32,
    pub chroma: u32,
}

impl Default for AlgoVersions {
    fn default() -> Self {
        Self {
            phash: 1,
            whash: 1,
            dhash: 1,
            ahash: 1,
            pdq: 1,
            chroma: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryExplanation {
    pub tier: u8,
    pub algorithm: String,
    pub algo_versions: AlgoVersions,
    pub score: f64,
    pub threshold: f64,
    #[serde(rename = "match")]
    pub match_status: bool,
    pub satisfies_tau: bool,
    pub breakdown: ScoreBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResultItem {
    pub rank: usize,
    pub score: f64,
    pub doc_id: i64,
    pub sha256: String,
    pub origin_uri: String,
    pub title: Option<String>,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub explanation: QueryExplanation,
}

#[derive(Debug, Clone)]
pub struct QueryOptions {
    pub limit: usize,
    pub min_score: f64,
    pub corpus: Option<String>,
}

pub struct QueryEngine<'a> {
    db: &'a Database,
    blob_store: &'a BlobStore,
    config: &'a Config,
}

impl<'a> QueryEngine<'a> {
    pub fn new(db: &'a Database, blob_store: &'a BlobStore, config: &'a Config) -> Self {
        Self {
            db,
            blob_store,
            config,
        }
    }

    pub fn query_file<P: AsRef<Path>>(
        &self,
        image_path: P,
        options: &QueryOptions,
    ) -> Result<Vec<QueryResultItem>> {
        let bytes = std::fs::read(image_path)?;
        self.query_bytes(&bytes, options)
    }

    pub fn query_bytes(
        &self,
        image_bytes: &[u8],
        options: &QueryOptions,
    ) -> Result<Vec<QueryResultItem>> {
        let norm = normalize(image_bytes, &self.config.normalize)?;

        let q_ahash = ahash_from_luma(
            norm.analysis.width,
            norm.analysis.height,
            &norm.analysis.luma,
        );
        let q_dhash = dhash_from_luma(
            norm.analysis.width,
            norm.analysis.height,
            &norm.analysis.luma,
        );
        let q_phash = phash_from_luma(
            norm.analysis.width,
            norm.analysis.height,
            &norm.analysis.luma,
        );
        let q_whash = whash_from_luma(
            norm.analysis.width,
            norm.analysis.height,
            &norm.analysis.luma,
        );

        let (q_pdq_hash, q_pdq_quality) = pdq_from_luma(
            norm.analysis.width,
            norm.analysis.height,
            &norm.analysis.luma,
        );

        let q_chroma = compute_color_histogram(&norm.analysis.rgb);

        let docs = if let Some(corpus) = &options.corpus {
            self.db.get_docs_by_corpus(corpus)?
        } else {
            self.db.get_all_docs()?
        };

        let weights = &self.config.engine.weights;

        let mut scored_items: Vec<(f64, Doc, ScoreBreakdown)> = Vec::new();

        for doc in docs {
            let cand_chroma = match self.blob_store.get(&doc.chroma_blob_ref) {
                Ok(chroma_bytes) if chroma_bytes.len() == 1024 => {
                    bytes_to_chroma(&chroma_bytes).ok()
                }
                _ => None,
            };

            let breakdown = compute_combined_score(
                q_phash,
                q_whash,
                q_dhash,
                q_ahash,
                &q_pdq_hash.0,
                q_pdq_quality,
                &q_chroma,
                doc.phash,
                doc.whash,
                doc.dhash,
                doc.ahash,
                &[0u8; 32],
                0,
                cand_chroma.as_ref(),
                weights,
                &self.config.engine.tau,
            );

            let score = breakdown.combined_score;

            if score >= options.min_score {
                scored_items.push((score, doc, breakdown));
            }
        }

        scored_items.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.doc_id.cmp(&b.1.doc_id))
        });

        let mut results = Vec::new();
        for (idx, (score, doc, breakdown)) in
            scored_items.into_iter().take(options.limit).enumerate()
        {
            let sightings = self.db.get_sightings_by_doc_id(doc.doc_id)?;
            let primary_sighting = sightings.into_iter().next();

            let origin_uri = primary_sighting
                .as_ref()
                .map(|s| s.origin_uri.clone())
                .unwrap_or_default();
            let title = primary_sighting.and_then(|s| s.title);

            let match_status =
                score >= self.config.engine.min_exact_score && breakdown.satisfies_tau;

            results.push(QueryResultItem {
                rank: idx + 1,
                score,
                doc_id: doc.doc_id,
                sha256: bytes_to_hex(&doc.sha256),
                origin_uri,
                title,
                format: doc.format,
                width: doc.width,
                height: doc.height,
                explanation: QueryExplanation {
                    tier: 1,
                    algorithm: "perceptual_hash_ensemble".to_string(),
                    algo_versions: AlgoVersions::default(),
                    score,
                    threshold: self.config.engine.min_exact_score,
                    match_status,
                    satisfies_tau: breakdown.satisfies_tau,
                    breakdown,
                },
            });
        }

        Ok(results)
    }
}
