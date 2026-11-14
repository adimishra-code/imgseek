use image::imageops::{self, FilterType};
use image::GrayImage;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub const PDQ_BLOCK_SIZE: usize = 64;
pub const PDQ_COEFF_SIZE: usize = 16;
pub const PDQ_HASH_BITS: usize = 256;
pub const PDQ_HASH_BYTES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PdqHash(pub [u8; PDQ_HASH_BYTES]);

impl PdqHash {
    pub const ZERO: Self = Self([0u8; PDQ_HASH_BYTES]);

    pub fn new(bytes: [u8; PDQ_HASH_BYTES]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; PDQ_HASH_BYTES] {
        &self.0
    }

    pub fn to_bytes(&self) -> [u8; PDQ_HASH_BYTES] {
        self.0
    }

    pub fn to_u64_array(&self) -> [u64; 4] {
        let b = &self.0;
        [
            u64::from_be_bytes(b[0..8].try_into().unwrap()),
            u64::from_be_bytes(b[8..16].try_into().unwrap()),
            u64::from_be_bytes(b[16..24].try_into().unwrap()),
            u64::from_be_bytes(b[24..32].try_into().unwrap()),
        ]
    }

    pub fn from_u64_array(words: [u64; 4]) -> Self {
        let mut bytes = [0u8; 32];
        bytes[0..8].copy_from_slice(&words[0].to_be_bytes());
        bytes[8..16].copy_from_slice(&words[1].to_be_bytes());
        bytes[16..24].copy_from_slice(&words[2].to_be_bytes());
        bytes[24..32].copy_from_slice(&words[3].to_be_bytes());
        Self(bytes)
    }

    pub fn hamming_distance(&self, other: &Self) -> u32 {
        let w1 = self.to_u64_array();
        let w2 = other.to_u64_array();
        (w1[0] ^ w2[0]).count_ones()
            + (w1[1] ^ w2[1]).count_ones()
            + (w1[2] ^ w2[2]).count_ones()
            + (w1[3] ^ w2[3]).count_ones()
    }

    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for byte in &self.0 {
            use std::fmt::Write;
            let _ = write!(s, "{:02x}", byte);
        }
        s
    }

    pub fn from_hex(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.len() != 64 {
            return Err(format!("expected 64 hex characters, found {}", s.len()));
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(|e| format!("invalid hex character at {}: {}", i * 2, e))?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Debug for PdqHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PdqHash({})", self.to_hex())
    }
}

impl fmt::Display for PdqHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl Serialize for PdqHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for PdqHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        PdqHash::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

pub fn compute_pdq_quality(mat_64: &[f64; 64 * 64]) -> u16 {
    let mut grad_sum: i64 = 0;

    // Horizontal gradients: 64 rows x 63 adjacent pairs
    for y in 0..64 {
        let row_offset = y * 64;
        for x in 0..63 {
            let u = mat_64[row_offset + x];
            let v = mat_64[row_offset + x + 1];
            let d = (((u - v) * 100.0) / 255.0).trunc() as i64;
            grad_sum += d.abs();
        }
    }

    // Vertical gradients: 63 row pairs x 64 columns
    for y in 0..63 {
        let row_offset_1 = y * 64;
        let row_offset_2 = (y + 1) * 64;
        for x in 0..64 {
            let u = mat_64[row_offset_1 + x];
            let v = mat_64[row_offset_2 + x];
            let d = (((u - v) * 100.0) / 255.0).trunc() as i64;
            grad_sum += d.abs();
        }
    }

    (grad_sum / 90).clamp(0, 100) as u16
}

pub fn pdq_from_matrix_64x64(mat_64: &[f64; 64 * 64]) -> (PdqHash, u16) {
    // Precomputed cosine factors: C[k][n] = 2.0 * cos(pi * (2n + 1) * k / 128)
    let mut cos_table = [[0.0f64; 64]; 16];
    for (k, row) in cos_table.iter_mut().enumerate() {
        for (n, val) in row.iter_mut().enumerate() {
            *val = 2.0 * ((std::f64::consts::PI * (2 * n + 1) as f64 * k as f64) / 128.0).cos();
        }
    }

    // 1D DCT on each of the 64 rows -> 64 x 16 intermediate coefficients
    let mut t_row = [[0.0f64; 16]; 64];
    for (y, t_row_y) in t_row.iter_mut().enumerate() {
        let row_offset = y * 64;
        for (kx, t_val) in t_row_y.iter_mut().enumerate() {
            let mut sum = 0.0f64;
            for (n, &cos_val) in cos_table[kx].iter().enumerate() {
                sum += cos_val * mat_64[row_offset + n];
            }
            *t_val = sum;
        }
    }

    // 1D DCT on each of the 16 columns -> 16 x 16 2D DCT coefficients
    let mut dct_16x16 = [0.0f64; 256];
    for kx in 0..16 {
        for ky in 0..16 {
            let mut sum = 0.0f64;
            for y in 0..64 {
                sum += cos_table[ky][y] * t_row[y][kx];
            }
            dct_16x16[ky * 16 + kx] = sum;
        }
    }

    // Calculate median across the 256 coefficients
    let mut sorted_coeffs = dct_16x16;
    sorted_coeffs.sort_by(|a, b| a.total_cmp(b));
    let median = (sorted_coeffs[127] + sorted_coeffs[128]) / 2.0;

    // Threshold against median and bit-pack into 32 bytes (256 bits)
    let mut hash_bytes = [0u8; 32];
    for (i, &coeff) in dct_16x16.iter().enumerate() {
        if coeff > median {
            hash_bytes[i / 8] |= 1 << (7 - (i % 8));
        }
    }

    let quality = compute_pdq_quality(mat_64);
    (PdqHash(hash_bytes), quality)
}

pub fn pdq_from_luma(width: u32, height: u32, luma: &[u8]) -> (PdqHash, u16) {
    assert_eq!(
        luma.len(),
        (width * height) as usize,
        "luma buffer length must equal width * height"
    );

    let mat_64 = if width == 64 && height == 64 {
        let mut m = [0.0f64; 64 * 64];
        for (i, &p) in luma.iter().enumerate() {
            m[i] = p as f64;
        }
        m
    } else {
        // Step 1: Downscale to 512x512 with bilinear/triangle filter
        let src_img = GrayImage::from_raw(width, height, luma.to_vec())
            .expect("valid GrayImage from luma buffer");
        let img_512 = imageops::resize(&src_img, 512, 512, FilterType::Triangle);

        // Step 2: 8x8 block box-filter downsampling from 512x512 to 64x64
        let mut m = [0.0f64; 64 * 64];
        let p512 = img_512.as_raw();
        for y in 0..64 {
            for x in 0..64 {
                let mut sum = 0.0f64;
                for dy in 0..8 {
                    let py = y * 8 + dy;
                    let row_start = py * 512;
                    for dx in 0..8 {
                        let px = x * 8 + dx;
                        sum += p512[row_start + px] as f64;
                    }
                }
                m[y * 64 + x] = sum / 64.0;
            }
        }
        m
    };

    pdq_from_matrix_64x64(&mat_64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pdq_hash_round_trip_conversions() {
        let hex_sample = "ac49b16ef78404a9684dc4efe7d0c3a15feec2ba583029d9b3cb999407af4a1a";
        let hash = PdqHash::from_hex(hex_sample).expect("parse hex");
        assert_eq!(hash.to_hex(), hex_sample);

        let words = hash.to_u64_array();
        let from_words = PdqHash::from_u64_array(words);
        assert_eq!(hash, from_words);

        let dist = hash.hamming_distance(&hash);
        assert_eq!(dist, 0);

        let mut flipped = hash.to_bytes();
        flipped[0] ^= 0x01; // flip 1 bit
        let hash2 = PdqHash(flipped);
        assert_eq!(hash.hamming_distance(&hash2), 1);
    }

    #[test]
    fn test_pdq_quality_extremes() {
        let flat = [128.0f64; 64 * 64];
        assert_eq!(compute_pdq_quality(&flat), 0);

        let mut checker = [0.0f64; 64 * 64];
        for y in 0..64 {
            for x in 0..64 {
                if (x + y) % 2 == 0 {
                    checker[y * 64 + x] = 255.0;
                }
            }
        }
        assert_eq!(compute_pdq_quality(&checker), 100);
    }

    #[test]
    fn test_pdq_layer_a_reference_exact() {
        // Pseudo-random deterministic 64x64 matrix matching Python test
        let mut mat_64 = [0.0f64; 64 * 64];
        let mut state: u64 = 1337;
        for item in mat_64.iter_mut() {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let val = ((state >> 33) % 256) as f64;
            *item = val;
        }

        let (hash, quality) = pdq_from_matrix_64x64(&mat_64);
        assert_eq!(hash.as_bytes().len(), 32);
        // Bit count should be approximately 128 (balanced median threshold)
        let ones: u32 = hash.as_bytes().iter().map(|b| b.count_ones()).sum();
        assert_eq!(ones, 128);
        assert!(quality > 50, "random matrix should have high quality");
    }

    #[test]
    fn test_layer_a_pdq_bit_exact() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let golden_path = manifest_dir.join("../../tests/fixtures/golden_hashes.json");
        let content = std::fs::read_to_string(&golden_path).expect("read golden_hashes.json");
        let records: serde_json::Value = serde_json::from_str(&content).expect("parse json");

        let mut checked = 0;
        for record in records.as_array().expect("array of records") {
            let layer_a = &record["layer_a"];
            let mat_raw = layer_a["mat_64x64"].as_array().expect("mat_64x64");
            let expected_pdq = layer_a["pdq"].as_str().expect("expected pdq");
            let expected_quality = layer_a["pdq_quality"]
                .as_u64()
                .expect("expected pdq_quality") as u16;

            let mut mat_64x64 = [0.0f64; 64 * 64];
            let mut idx = 0;
            for row in mat_raw {
                for cell in row.as_array().expect("row") {
                    mat_64x64[idx] = cell.as_u64().expect("cell") as f64;
                    idx += 1;
                }
            }

            let (computed_hash, computed_quality) = pdq_from_matrix_64x64(&mat_64x64);
            assert_eq!(
                computed_hash.to_hex(),
                expected_pdq,
                "Layer-A PDQ hash bit mismatch for record id: {}",
                record["id"]
            );
            assert_eq!(
                computed_quality, expected_quality,
                "Layer-A PDQ quality mismatch for record id: {}",
                record["id"]
            );
            checked += 1;
        }

        assert_eq!(checked, 200, "Must check all 200 golden records");
    }
}
