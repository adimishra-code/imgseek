use image::imageops::{self, FilterType};

use crate::composite::composite_onto_white;
use crate::config::NormalizeConfig;
use crate::decode::decode_image;
use crate::error::Result;
use crate::orient::{apply_orientation, parse_exif_orientation};

/// The downscaled view used for local features (ORB) and chroma histograms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisView {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub luma: Vec<u8>,
}

/// The canonical normalised image container produced by the ingest/query pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub luma: Vec<u8>,
    pub analysis: AnalysisView,
}

/// Converts RGB8 pixel slice to luma using ITU-R 601 integer arithmetic matching PIL:
/// Y = (R*19595 + G*38470 + B*7471 + 0x8000) >> 16
#[inline]
pub fn rgb_to_luma_byte(r: u8, g: u8, b: u8) -> u8 {
    let y = (r as u32 * 19595 + g as u32 * 38470 + b as u32 * 7471 + 0x8000) >> 16;
    y as u8
}

/// Computes luma buffer from an RGB8 buffer.
pub fn compute_luma_from_rgb(rgb: &[u8]) -> Vec<u8> {
    let count = rgb.len() / 3;
    let mut luma = Vec::with_capacity(count);
    let (chunks, _) = rgb.as_chunks::<3>();
    for chunk in chunks {
        luma.push(rgb_to_luma_byte(chunk[0], chunk[1], chunk[2]));
    }
    luma
}

/// Runs the full canonical normalisation pipeline:
/// 1. Magic bytes sniff
/// 2. Header dimension check against pixel budget
/// 3. Decode
/// 4. EXIF orientation (JPEG/TIFF)
/// 5. Alpha composited onto white, converted to 8-bit sRGB
/// 6. Full-resolution integer luma
/// 7. Analysis view (longest side <= analysis_max_side, Lanczos3, never upscaled)
pub fn normalize(bytes: &[u8], config: &NormalizeConfig) -> Result<NormalizedImage> {
    let (decoded, format) = decode_image(bytes, config.max_pixels)?;

    let oriented = if let Some(orientation) = parse_exif_orientation(bytes, format) {
        apply_orientation(decoded, orientation)
    } else {
        decoded
    };

    let rgb_image = composite_onto_white(oriented);
    let width = rgb_image.width();
    let height = rgb_image.height();
    let rgb = rgb_image.clone().into_raw();
    let luma = compute_luma_from_rgb(&rgb);

    let max_side = width.max(height);
    let analysis = if max_side <= config.analysis_max_side {
        AnalysisView {
            width,
            height,
            rgb: rgb.clone(),
            luma: luma.clone(),
        }
    } else {
        let (target_w, target_h) = if width >= height {
            let tw = config.analysis_max_side;
            let th = ((height as u64 * config.analysis_max_side as u64 + width as u64 / 2)
                / width as u64)
                .max(1) as u32;
            (tw, th)
        } else {
            let th = config.analysis_max_side;
            let tw = ((width as u64 * config.analysis_max_side as u64 + height as u64 / 2)
                / height as u64)
                .max(1) as u32;
            (tw, th)
        };

        let resized_rgb_img =
            imageops::resize(&rgb_image, target_w, target_h, FilterType::Lanczos3);
        let analysis_rgb = resized_rgb_img.into_raw();
        let analysis_luma = compute_luma_from_rgb(&analysis_rgb);

        AnalysisView {
            width: target_w,
            height: target_h,
            rgb: analysis_rgb,
            luma: analysis_luma,
        }
    };

    Ok(NormalizedImage {
        width,
        height,
        rgb,
        luma,
        analysis,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use std::io::Cursor;

    fn make_test_png(width: u32, height: u32, color: [u8; 3]) -> Vec<u8> {
        let img = RgbImage::from_pixel(width, height, Rgb(color));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn test_pil_integer_luma_weights() {
        assert_eq!(rgb_to_luma_byte(0, 0, 0), 0);
        assert_eq!(rgb_to_luma_byte(255, 255, 255), 255);
        // Pure red: 255 * 19595 + 32768 = 5029493 >> 16 = 76
        assert_eq!(rgb_to_luma_byte(255, 0, 0), 76);
        // Pure green: 255 * 38470 + 32768 = 9842618 >> 16 = 150
        assert_eq!(rgb_to_luma_byte(0, 255, 0), 150);
        // Pure blue: 255 * 7471 + 32768 = 1937873 >> 16 = 29
        assert_eq!(rgb_to_luma_byte(0, 0, 255), 29);
    }

    #[test]
    fn test_normalize_small_image_no_downscale() {
        let png = make_test_png(100, 50, [100, 150, 200]);
        let cfg = NormalizeConfig::default();
        let norm = normalize(&png, &cfg).unwrap();

        assert_eq!(norm.width, 100);
        assert_eq!(norm.height, 50);
        assert_eq!(norm.analysis.width, 100);
        assert_eq!(norm.analysis.height, 50);
        assert_eq!(norm.rgb.len(), 100 * 50 * 3);
        assert_eq!(norm.luma.len(), 100 * 50);
    }

    #[test]
    fn test_normalize_large_image_downscale_to_1024() {
        let png = make_test_png(2000, 1000, [50, 100, 150]);
        let cfg = NormalizeConfig::default();
        let norm = normalize(&png, &cfg).unwrap();

        assert_eq!(norm.width, 2000);
        assert_eq!(norm.height, 1000);
        assert_eq!(norm.analysis.width, 1024);
        assert_eq!(norm.analysis.height, 512);
        assert_eq!(norm.analysis.rgb.len(), 1024 * 512 * 3);
        assert_eq!(norm.analysis.luma.len(), 1024 * 512);
    }
}
