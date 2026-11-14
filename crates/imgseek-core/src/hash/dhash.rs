use image::imageops::{self, FilterType};
use image::GrayImage;

pub fn dhash_from_9x8(pixels: &[u8; 72]) -> u64 {
    let mut hash = 0u64;
    for r in 0..8 {
        let row_start = r * 9;
        for c in 0..8 {
            let left = pixels[row_start + c];
            let right = pixels[row_start + c + 1];
            hash = (hash << 1) | (if right > left { 1 } else { 0 });
        }
    }
    hash
}

pub fn dhash_from_luma(width: u32, height: u32, luma: &[u8]) -> u64 {
    let gray_img = GrayImage::from_raw(width, height, luma.to_vec())
        .expect("luma buffer size must match width * height");
    let resized = imageops::resize(&gray_img, 9, 8, FilterType::Lanczos3);
    let mut mat = [0u8; 72];
    mat.copy_from_slice(resized.as_raw());
    dhash_from_9x8(&mat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_layer_a_dhash_bit_exact() {
        let golden_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/golden_hashes.json"
        );
        let content = std::fs::read_to_string(golden_path).expect("read golden_hashes.json");
        let records: Vec<Value> = serde_json::from_str(&content).expect("parse JSON");

        let mut checked = 0;
        for record in records {
            let layer_a = &record["layer_a"];
            let mat_raw = layer_a["mat_9x8"].as_array().expect("mat_9x8");
            let expected_hash = layer_a["dhash"].as_u64().expect("expected dhash");

            let mut mat_9x8 = [0u8; 72];
            let mut idx = 0;
            for row in mat_raw {
                for cell in row.as_array().expect("row") {
                    mat_9x8[idx] = cell.as_u64().expect("cell") as u8;
                    idx += 1;
                }
            }

            let computed = dhash_from_9x8(&mat_9x8);
            assert_eq!(
                computed, expected_hash,
                "Layer-A dHash bit mismatch for record id: {}",
                record["id"]
            );
            checked += 1;
        }

        assert!(checked >= 200, "Must check at least 200 golden records");
    }
}
