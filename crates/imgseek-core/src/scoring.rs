use crate::chroma::histogram_distance;
use crate::config::{TauConfig, WeightsConfig};
use serde::{Deserialize, Serialize};

/// Detailed breakdown of similarity metrics, distances, and weights across all algorithms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub phash_distance: u32,
    pub phash_similarity: f64,
    pub phash_weight: f64,

    pub whash_distance: u32,
    pub whash_similarity: f64,
    pub whash_weight: f64,

    pub dhash_distance: u32,
    pub dhash_similarity: f64,
    pub dhash_weight: f64,

    pub ahash_distance: u32,
    pub ahash_similarity: f64,
    pub ahash_weight: f64,

    pub pdq_distance: u32,
    pub pdq_similarity: f64,
    pub pdq_weight: f64,
    pub pdq_effective_quality: u16,

    pub chroma_distance: f64,
    pub chroma_similarity: f64,
    pub chroma_weight: f64,

    pub total_weight: f64,
    pub combined_score: f64,
    pub satisfies_tau: bool,
    pub satisfies_all_tau: bool,
}

/// Compute the combined similarity score and full breakdown for a candidate document.
///
/// Features:
/// - 64-bit normalized similarities: $1.0 - (d / 64.0)$
/// - 256-bit PDQ normalized similarity: $1.0 - (d / 256.0)$
/// - PDQ quality down-weighting: if effective quality $Q < 50$, PDQ weight is scaled by $(Q / 50.0)$
/// - Tau limits check: verifies whether candidate meets perceptual hash tau limits
/// - Weighted combination normalized by sum of active weights
#[allow(clippy::too_many_arguments)]
pub fn compute_combined_score(
    q_phash: u64,
    q_whash: u64,
    q_dhash: u64,
    q_ahash: u64,
    q_pdq: &[u8; 32],
    q_pdq_quality: u16,
    q_chroma: &[f32; 256],
    t_phash: u64,
    t_whash: u64,
    t_dhash: u64,
    t_ahash: u64,
    t_pdq: &[u8; 32],
    t_pdq_quality: u16,
    t_chroma: Option<&[f32; 256]>,
    weights: &WeightsConfig,
    tau: &TauConfig,
) -> ScoreBreakdown {
    let phash_distance = (q_phash ^ t_phash).count_ones();
    let whash_distance = (q_whash ^ t_whash).count_ones();
    let dhash_distance = (q_dhash ^ t_dhash).count_ones();
    let ahash_distance = (q_ahash ^ t_ahash).count_ones();

    let mut pdq_distance = 0u32;
    for i in 0..32 {
        pdq_distance += (q_pdq[i] ^ t_pdq[i]).count_ones();
    }

    let chroma_distance = match t_chroma {
        Some(tc) => histogram_distance(q_chroma, tc) as f64,
        None => 1.0,
    };

    let phash_similarity = 1.0 - (phash_distance as f64 / 64.0);
    let whash_similarity = 1.0 - (whash_distance as f64 / 64.0);
    let dhash_similarity = 1.0 - (dhash_distance as f64 / 64.0);
    let ahash_similarity = 1.0 - (ahash_distance as f64 / 64.0);
    let pdq_similarity = 1.0 - (pdq_distance as f64 / 256.0);
    let chroma_similarity = (1.0 - chroma_distance).clamp(0.0, 1.0);

    // Down-weight PDQ if quality < 50
    let pdq_effective_quality = q_pdq_quality.min(t_pdq_quality);
    let pdq_weight = if pdq_effective_quality < 50 {
        weights.pdq * (pdq_effective_quality as f64 / 50.0)
    } else {
        weights.pdq
    };

    let total_weight =
        weights.phash + weights.whash + weights.dhash + weights.ahash + pdq_weight + weights.chroma;

    let combined_score = if total_weight > 0.0 {
        (weights.phash * phash_similarity
            + weights.whash * whash_similarity
            + weights.dhash * dhash_similarity
            + weights.ahash * ahash_similarity
            + pdq_weight * pdq_similarity
            + weights.chroma * chroma_similarity)
            / total_weight
    } else {
        0.0
    };

    let phash_ok = phash_distance <= tau.phash;
    let whash_ok = whash_distance <= tau.whash;
    let dhash_ok = dhash_distance <= tau.dhash;
    let ahash_ok = ahash_distance <= tau.ahash;
    let pdq_ok = pdq_distance <= tau.pdq;

    // Satisfies tau if at least one primary hash is within threshold
    let satisfies_tau = phash_ok || whash_ok || dhash_ok || pdq_ok;
    // Satisfies all tau if all hashes are within threshold
    let satisfies_all_tau = phash_ok && whash_ok && dhash_ok && ahash_ok && pdq_ok;

    ScoreBreakdown {
        phash_distance,
        phash_similarity,
        phash_weight: weights.phash,
        whash_distance,
        whash_similarity,
        whash_weight: weights.whash,
        dhash_distance,
        dhash_similarity,
        dhash_weight: weights.dhash,
        ahash_distance,
        ahash_similarity,
        ahash_weight: weights.ahash,
        pdq_distance,
        pdq_similarity,
        pdq_weight,
        pdq_effective_quality,
        chroma_distance,
        chroma_similarity,
        chroma_weight: weights.chroma,
        total_weight,
        combined_score,
        satisfies_tau,
        satisfies_all_tau,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_identical_hashes_yield_perfect_score() {
        let weights = WeightsConfig::default();
        let tau = TauConfig::default();

        let phash = 0x1234_5678_9ABC_DEF0;
        let whash = 0xCAFE_BABE_DEAD_BEEF;
        let dhash = 0x0102_0304_0506_0708;
        let ahash = 0xFEDC_BA98_7654_3210;
        let pdq = [0x55; 32];
        let chroma = [1.0 / 256.0; 256];

        let breakdown = compute_combined_score(
            phash,
            whash,
            dhash,
            ahash,
            &pdq,
            95,
            &chroma,
            phash,
            whash,
            dhash,
            ahash,
            &pdq,
            95,
            Some(&chroma),
            &weights,
            &tau,
        );

        assert_eq!(breakdown.phash_distance, 0);
        assert_eq!(breakdown.whash_distance, 0);
        assert_eq!(breakdown.dhash_distance, 0);
        assert_eq!(breakdown.ahash_distance, 0);
        assert_eq!(breakdown.pdq_distance, 0);
        assert!((breakdown.chroma_distance - 0.0).abs() < 1e-6);
        assert!((breakdown.combined_score - 1.0).abs() < 1e-6);
        assert!(breakdown.satisfies_tau);
        assert!(breakdown.satisfies_all_tau);
    }

    #[test]
    fn test_pdq_quality_downweighting() {
        let weights = WeightsConfig::default();
        let tau = TauConfig::default();

        let h = 0u64;
        let pdq = [0u8; 32];
        let chroma = [1.0 / 256.0; 256];

        // High quality (>= 50): full weight
        let high_q = compute_combined_score(
            h,
            h,
            h,
            h,
            &pdq,
            80,
            &chroma,
            h,
            h,
            h,
            h,
            &pdq,
            80,
            Some(&chroma),
            &weights,
            &tau,
        );
        assert_eq!(high_q.pdq_weight, weights.pdq);

        // Low quality (25 < 50): half weight (25 / 50 = 0.5)
        let low_q = compute_combined_score(
            h,
            h,
            h,
            h,
            &pdq,
            25,
            &chroma,
            h,
            h,
            h,
            h,
            &pdq,
            25,
            Some(&chroma),
            &weights,
            &tau,
        );
        assert!((low_q.pdq_weight - (weights.pdq * 0.5)).abs() < 1e-6);

        // Zero quality (0): zero weight
        let zero_q = compute_combined_score(
            h,
            h,
            h,
            h,
            &pdq,
            0,
            &chroma,
            h,
            h,
            h,
            h,
            &pdq,
            0,
            Some(&chroma),
            &weights,
            &tau,
        );
        assert_eq!(zero_q.pdq_weight, 0.0);
    }

    #[test]
    fn test_tau_limits_satisfaction() {
        let weights = WeightsConfig::default();
        let tau = TauConfig::default();

        let h1 = 0u64;
        let pdq1 = [0u8; 32];
        let chroma = [1.0 / 256.0; 256];

        // h2 with phash distance = 13 (> tau.phash = 12), but whash distance = 0 (<= tau.whash = 10)
        let phash2 = 0x0000_0000_0000_1FFF; // 13 bits set
        let whash2 = 0;

        let breakdown = compute_combined_score(
            h1,
            h1,
            h1,
            h1,
            &pdq1,
            80,
            &chroma,
            phash2,
            whash2,
            h1,
            h1,
            &pdq1,
            80,
            Some(&chroma),
            &weights,
            &tau,
        );

        assert_eq!(breakdown.phash_distance, 13);
        assert_eq!(breakdown.whash_distance, 0);
        assert!(breakdown.satisfies_tau); // whash satisfies tau
        assert!(!breakdown.satisfies_all_tau); // phash fails tau
    }
}
