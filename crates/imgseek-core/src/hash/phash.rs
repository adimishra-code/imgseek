use image::imageops::{self, FilterType};
use image::GrayImage;

const N: usize = 32;

const fn bit_reverse_5(mut x: usize) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < 5 {
        n = (n << 1) | (x & 1);
        x >>= 1;
        i += 1;
    }
    n
}

pub fn dct_1d_32_to_8(x: &[f64; N]) -> [f64; 8] {
    let mut re = [0.0f64; N];
    let mut im = [0.0f64; N];
    for n in 0..(N / 2) {
        re[n] = x[2 * n];
        re[N - 1 - n] = x[2 * n + 1];
    }

    for i in 0..N {
        let j = bit_reverse_5(i);
        if i < j {
            re.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= N {
        let half = len / 2;
        let angle = -2.0 * std::f64::consts::PI / (len as f64);
        let w_step_re = angle.cos();
        let w_step_im = angle.sin();

        let mut i = 0;
        while i < N {
            let mut w_re = 1.0f64;
            let mut w_im = 0.0f64;
            for j in 0..half {
                let u_re = re[i + j];
                let u_im = im[i + j];
                let v_re = re[i + j + half] * w_re - im[i + j + half] * w_im;
                let v_im = re[i + j + half] * w_im + im[i + j + half] * w_re;

                re[i + j] = u_re + v_re;
                im[i + j] = u_im + v_im;
                re[i + j + half] = u_re - v_re;
                im[i + j + half] = u_im - v_im;

                let next_w_re = w_re * w_step_re - w_im * w_step_im;
                let next_w_im = w_re * w_step_im + w_im * w_step_re;
                w_re = next_w_re;
                w_im = next_w_im;
            }
            i += len;
        }
        len *= 2;
    }

    let mut out = [0.0f64; 8];
    for (k, cell) in out.iter_mut().enumerate() {
        let theta = std::f64::consts::PI * (k as f64) / 64.0;
        let cos_theta = theta.cos();
        let sin_theta = theta.sin();
        *cell = 2.0 * (re[k] * cos_theta + im[k] * sin_theta);
    }
    out
}

pub fn phash_from_32x32(pixels: &[u8; 1024]) -> u64 {
    let mut step1 = [[0.0f64; 8]; N];
    let mut col_buf = [0.0f64; N];
    for c in 0..N {
        for r in 0..N {
            col_buf[r] = pixels[r * N + c] as f64;
        }
        step1[c] = dct_1d_32_to_8(&col_buf);
    }

    let mut dct_8x8 = [[0.0f64; 8]; 8];
    let mut row_buf = [0.0f64; N];
    let mut values_64 = [0.0f64; 64];

    for r in 0..8 {
        for c in 0..N {
            row_buf[c] = step1[c][r];
        }
        let row_freqs = dct_1d_32_to_8(&row_buf);
        dct_8x8[r] = row_freqs;
        for c in 0..8 {
            values_64[r * 8 + c] = row_freqs[c];
        }
    }

    let mut sorted_values = values_64;
    sorted_values.sort_by(|a, b| a.total_cmp(b));
    let median = (sorted_values[31] + sorted_values[32]) / 2.0;

    let mut hash = 0u64;
    for row in &dct_8x8 {
        for &val in row {
            hash = (hash << 1) | (if val > median { 1 } else { 0 });
        }
    }

    hash
}

pub fn phash_from_luma(width: u32, height: u32, luma: &[u8]) -> u64 {
    let gray_img = GrayImage::from_raw(width, height, luma.to_vec())
        .expect("luma buffer size must match width * height");
    let resized = imageops::resize(&gray_img, 32, 32, FilterType::Lanczos3);
    let mut mat = [0u8; 1024];
    mat.copy_from_slice(resized.as_raw());
    phash_from_32x32(&mat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_layer_a_phash_bit_exact() {
        let golden_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/golden_hashes.json"
        );
        let content = std::fs::read_to_string(golden_path).expect("read golden_hashes.json");
        let records: Vec<Value> = serde_json::from_str(&content).expect("parse JSON");

        let mut checked = 0;
        for record in records {
            let layer_a = &record["layer_a"];
            let mat_raw = layer_a["mat_32x32"].as_array().expect("mat_32x32");
            let expected_hash = layer_a["phash"].as_u64().expect("expected phash");

            let mut mat_32x32 = [0u8; 1024];
            let mut idx = 0;
            for row in mat_raw {
                for cell in row.as_array().expect("row") {
                    mat_32x32[idx] = cell.as_u64().expect("cell") as u8;
                    idx += 1;
                }
            }

            let computed = phash_from_32x32(&mat_32x32);
            assert_eq!(
                computed, expected_hash,
                "Layer-A pHash bit mismatch for record id: {}",
                record["id"]
            );
            checked += 1;
        }

        assert!(checked >= 200, "Must check at least 200 golden records");
    }
}
