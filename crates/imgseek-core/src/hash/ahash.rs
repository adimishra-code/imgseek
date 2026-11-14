use image::imageops::{self, FilterType};
use image::GrayImage;

/// Computes aHash (average hash) from an already-resized 8x8 luma array (64 bytes).
/// Bit order: row-major, element 0 is MSB of u64.
/// Logic: bit = pixel > mean.
pub fn ahash_from_8x8(pixels: &[u8; 64]) -> u64 {
    let sum: u64 = pixels.iter().map(|&p| p as u64).sum();
    let mean = sum as f64 / 64.0;

    let mut hash = 0u64;
    for &p in pixels {
        hash = (hash << 1) | (if (p as f64) > mean { 1 } else { 0 });
    }
    hash
}

/// Computes aHash from arbitrary width x height luma buffer by resizing to 8x8 with Lanczos3.
pub fn ahash_from_luma(width: u32, height: u32, luma: &[u8]) -> u64 {
    let gray_img = GrayImage::from_raw(width, height, luma.to_vec())
        .expect("luma buffer size must match width * height");
    let resized = imageops::resize(&gray_img, 8, 8, FilterType::Lanczos3);
    let mut mat = [0u8; 64];
    mat.copy_from_slice(resized.as_raw());
    ahash_from_8x8(&mat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_layer_a_ahash_bit_exact() {
        let golden_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/golden_hashes.json"
        );
        let content = std::fs::read_to_string(golden_path).expect("read golden_hashes.json");
        let records: Vec<Value> = serde_json::from_str(&content).expect("parse JSON");

        let mut checked = 0;
        for record in records {
            let layer_a = &record["layer_a"];
            let mat_raw = layer_a["mat_8x8"].as_array().expect("mat_8x8");
            let expected_hash = layer_a["ahash"].as_u64().expect("expected ahash");

            let mut mat_8x8 = [0u8; 64];
            let mut idx = 0;
            for row in mat_raw {
                for cell in row.as_array().expect("row") {
                    mat_8x8[idx] = cell.as_u64().expect("cell") as u8;
                    idx += 1;
                }
            }

            let computed = ahash_from_8x8(&mat_8x8);
            assert_eq!(
                computed, expected_hash,
                "Layer-A aHash bit mismatch for record id: {}",
                record["id"]
            );
            checked += 1;
        }

        assert!(checked >= 200, "Must check at least 200 golden records");
    }
}
