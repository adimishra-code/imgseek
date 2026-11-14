use std::path::Path;

use imgseek_core::{
    ahash_from_luma, compute_color_histogram, dhash_from_luma, histogram_distance, normalize,
    phash_from_luma, whash_from_luma, Config,
};
use imgseek_store::{bytes_to_hex, BlobStore, Database, Doc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub phash_distance: u32,
    pub phash_weight: f64,
    pub whash_distance: u32,
    pub whash_weight: f64,
    pub dhash_distance: u32,
    pub dhash_weight: f64,
    pub ahash_distance: u32,
    pub ahash_weight: f64,
    pub chroma_distance: f64,
    pub chroma_weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryExplanation {
    pub tier: u8,
    pub algorithm: String,
    pub score: f64,
    pub threshold: f64,
    #[serde(rename = "match")]
    pub match_status: bool,
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

        let q_chroma = compute_color_histogram(&norm.analysis.rgb);

        let docs = if let Some(corpus) = &options.corpus {
            self.db.get_docs_by_corpus(corpus)?
        } else {
            self.db.get_all_docs()?
        };

        let weights = &self.config.engine.weights;
        let wp = weights.phash;
        let ww = weights.whash;
        let wd = weights.dhash;
        let wa = weights.ahash;
        let wc = weights.chroma;
        let total_weight = wp + ww + wd + wa + wc;

        let mut scored_items: Vec<(f64, Doc, ScoreBreakdown)> = Vec::new();

        for doc in docs {
            let phash_dist = (q_phash ^ doc.phash).count_ones();
            let whash_dist = (q_whash ^ doc.whash).count_ones();
            let dhash_dist = (q_dhash ^ doc.dhash).count_ones();
            let ahash_dist = (q_ahash ^ doc.ahash).count_ones();

            let chroma_dist = match self.blob_store.get(&doc.chroma_blob_ref) {
                Ok(chroma_bytes) if chroma_bytes.len() == 256 * 4 => {
                    let mut cand_chroma = [0f32; 256];
                    for (i, chunk) in chroma_bytes.as_chunks::<4>().0.iter().enumerate() {
                        cand_chroma[i] = f32::from_le_bytes(*chunk);
                    }
                    histogram_distance(&q_chroma, &cand_chroma) as f64
                }
                _ => 1.0,
            };

            let phash_sim = 1.0 - (phash_dist as f64 / 64.0);
            let whash_sim = 1.0 - (whash_dist as f64 / 64.0);
            let dhash_sim = 1.0 - (dhash_dist as f64 / 64.0);
            let ahash_sim = 1.0 - (ahash_dist as f64 / 64.0);
            let chroma_sim = 1.0 - chroma_dist;

            let score = (wp * phash_sim
                + ww * whash_sim
                + wd * dhash_sim
                + wa * ahash_sim
                + wc * chroma_sim)
                / total_weight;

            if score >= options.min_score {
                scored_items.push((
                    score,
                    doc,
                    ScoreBreakdown {
                        phash_distance: phash_dist,
                        phash_weight: wp,
                        whash_distance: whash_dist,
                        whash_weight: ww,
                        dhash_distance: dhash_dist,
                        dhash_weight: wd,
                        ahash_distance: ahash_dist,
                        ahash_weight: wa,
                        chroma_distance: chroma_dist,
                        chroma_weight: wc,
                    },
                ));
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
                    score,
                    threshold: options.min_score,
                    match_status: true,
                    breakdown,
                },
            });
        }

        Ok(results)
    }
}
