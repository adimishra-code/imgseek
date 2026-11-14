use image::imageops::{self, FilterType};
use image::GrayImage;

const S: f64 = std::f64::consts::FRAC_1_SQRT_2;
const NEG_S: f64 = -std::f64::consts::FRAC_1_SQRT_2;

#[inline]
fn dwt_1d(src: &[f64], ca: &mut [f64], cd: &mut [f64]) {
    let n = ca.len();
    for i in 0..n {
        let x0 = src[2 * i];
        let x1 = src[2 * i + 1];
        ca[i] = x0 * S + x1 * S;
        cd[i] = x0 * S + x1 * NEG_S;
    }
}

#[inline]
fn idwt_1d(ca: &[f64], cd: &[f64], dst: &mut [f64]) {
    let n = ca.len();
    for i in 0..n {
        let a = ca[i];
        let d = cd[i];
        dst[2 * i] = a * S + d * S;
        dst[2 * i + 1] = a * S + d * NEG_S;
    }
}

struct Details {
    da: Vec<f64>,
    ad: Vec<f64>,
    dd: Vec<f64>,
    h2: usize,
    w2: usize,
}

fn dwt_2d(mat: &[f64], h: usize, w: usize) -> (Vec<f64>, Details) {
    let h2 = h / 2;
    let w2 = w / 2;

    let mut col_a = vec![0.0f64; h2 * w];
    let mut col_d = vec![0.0f64; h2 * w];
    let mut src_col = vec![0.0f64; h];
    let mut ca_col = vec![0.0f64; h2];
    let mut cd_col = vec![0.0f64; h2];

    for c in 0..w {
        for r in 0..h {
            src_col[r] = mat[r * w + c];
        }
        dwt_1d(&src_col, &mut ca_col, &mut cd_col);
        for r in 0..h2 {
            col_a[r * w + c] = ca_col[r];
            col_d[r * w + c] = cd_col[r];
        }
    }

    let mut aa = vec![0.0f64; h2 * w2];
    let mut ad = vec![0.0f64; h2 * w2];
    let mut da = vec![0.0f64; h2 * w2];
    let mut dd = vec![0.0f64; h2 * w2];

    for r in 0..h2 {
        let row_a_slice = &col_a[r * w..r * w + w];
        let row_d_slice = &col_d[r * w..r * w + w];

        dwt_1d(
            row_a_slice,
            &mut aa[r * w2..r * w2 + w2],
            &mut ad[r * w2..r * w2 + w2],
        );
        dwt_1d(
            row_d_slice,
            &mut da[r * w2..r * w2 + w2],
            &mut dd[r * w2..r * w2 + w2],
        );
    }

    (aa, Details { da, ad, dd, h2, w2 })
}

fn idwt_2d(ll: &[f64], details: &Details) -> Vec<f64> {
    let h2 = details.h2;
    let w2 = details.w2;
    let h = h2 * 2;
    let w = w2 * 2;

    let mut col_a = vec![0.0f64; h2 * w];
    let mut col_d = vec![0.0f64; h2 * w];

    for r in 0..h2 {
        let ll_slice = &ll[r * w2..r * w2 + w2];
        let ad_slice = &details.ad[r * w2..r * w2 + w2];
        let da_slice = &details.da[r * w2..r * w2 + w2];
        let dd_slice = &details.dd[r * w2..r * w2 + w2];

        idwt_1d(ll_slice, ad_slice, &mut col_a[r * w..r * w + w]);
        idwt_1d(da_slice, dd_slice, &mut col_d[r * w..r * w + w]);
    }

    let mut out = vec![0.0f64; h * w];
    let mut ca_col = vec![0.0f64; h2];
    let mut cd_col = vec![0.0f64; h2];
    let mut dst_col = vec![0.0f64; h];

    for c in 0..w {
        for r in 0..h2 {
            ca_col[r] = col_a[r * w + c];
            cd_col[r] = col_d[r * w + c];
        }
        idwt_1d(&ca_col, &cd_col, &mut dst_col);
        for r in 0..h {
            out[r * w + c] = dst_col[r];
        }
    }

    out
}

pub fn whash_from_64x64(pixels: &[u8; 4096]) -> u64 {
    let mut cur: Vec<f64> = pixels.iter().map(|&p| p as f64).collect();
    let mut size = 64;
    let mut details_list = Vec::with_capacity(6);

    for _ in 0..6 {
        let (next_cur, details) = dwt_2d(&cur, size, size);
        cur = next_cur;
        size /= 2;
        details_list.push(details);
    }

    cur = vec![0.0f64; 1];
    for details in details_list.into_iter().rev() {
        cur = idwt_2d(&cur, &details);
    }

    size = 64;
    for _ in 0..3 {
        let (next_cur, _) = dwt_2d(&cur, size, size);
        cur = next_cur;
        size /= 2;
    }

    let mut sorted = cur.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = (sorted[31] + sorted[32]) / 2.0;

    let mut hash = 0u64;
    for &val in &cur {
        hash = (hash << 1) | (if val > median { 1 } else { 0 });
    }

    hash
}

pub fn whash_from_luma(width: u32, height: u32, luma: &[u8]) -> u64 {
    let gray_img = GrayImage::from_raw(width, height, luma.to_vec())
        .expect("luma buffer size must match width * height");
    let resized = imageops::resize(&gray_img, 64, 64, FilterType::Lanczos3);
    let mut mat = [0u8; 4096];
    mat.copy_from_slice(resized.as_raw());
    whash_from_64x64(&mat)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn test_layer_a_whash_bit_exact() {
        let golden_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/golden_hashes.json"
        );
        let content = std::fs::read_to_string(golden_path).expect("read golden_hashes.json");
        let records: Vec<Value> = serde_json::from_str(&content).expect("parse JSON");

        let mut checked = 0;
        for record in records {
            let layer_a = &record["layer_a"];
            let mat_raw = layer_a["mat_64x64"].as_array().expect("mat_64x64");
            let expected_hash = layer_a["whash"].as_u64().expect("expected whash");

            let mut mat_64x64 = [0u8; 4096];
            let mut idx = 0;
            for row in mat_raw {
                for cell in row.as_array().expect("row") {
                    mat_64x64[idx] = cell.as_u64().expect("cell") as u8;
                    idx += 1;
                }
            }

            let computed = whash_from_64x64(&mat_64x64);
            assert_eq!(
                computed, expected_hash,
                "Layer-A wHash bit mismatch for record id: {}",
                record["id"]
            );
            checked += 1;
        }

        assert!(checked >= 200, "Must check at least 200 golden records");
    }
}
