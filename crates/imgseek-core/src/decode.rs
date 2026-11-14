use crate::error::{CoreError, Result};
use crate::format::{sniff_format, ImageFormat};
use image::{DynamicImage, ImageFormat as ImgFormat, ImageReader};
use std::io::Cursor;

pub fn map_format(fmt: ImageFormat) -> ImgFormat {
    match fmt {
        ImageFormat::Jpeg => ImgFormat::Jpeg,
        ImageFormat::Png => ImgFormat::Png,
        ImageFormat::WebP => ImgFormat::WebP,
        ImageFormat::Gif => ImgFormat::Gif,
        ImageFormat::Bmp => ImgFormat::Bmp,
        ImageFormat::Tiff => ImgFormat::Tiff,
    }
}

pub fn decode_image(bytes: &[u8], max_pixels: u64) -> Result<(DynamicImage, ImageFormat)> {
    let fmt = sniff_format(bytes)?;
    let img_fmt = map_format(fmt);

    let reader = ImageReader::with_format(Cursor::new(bytes), img_fmt);
    let (width, height) = reader.into_dimensions().map_err(|e| {
        CoreError::DecodeError(format!("failed to read image header dimensions: {e}"))
    })?;

    let total_pixels = (width as u64).saturating_mul(height as u64);
    if total_pixels > max_pixels {
        return Err(CoreError::PixelBudgetExceeded {
            width,
            height,
            limit: max_pixels,
        });
    }

    let mut reader = ImageReader::with_format(Cursor::new(bytes), img_fmt);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    reader.limits(limits);

    let img = reader
        .decode()
        .map_err(|e| CoreError::DecodeError(format!("failed to decode image data: {e}")))?;

    Ok((img, fmt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    fn make_test_png(width: u32, height: u32) -> Vec<u8> {
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::new(width, height);
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        buf
    }

    #[test]
    fn test_decode_valid_image() {
        let png_bytes = make_test_png(10, 10);
        let (img, fmt) = decode_image(&png_bytes, 1000).unwrap();
        assert_eq!(fmt, ImageFormat::Png);
        assert_eq!(img.width(), 10);
        assert_eq!(img.height(), 10);
    }

    #[test]
    fn test_pixel_budget_exceeded() {
        let png_bytes = make_test_png(100, 100);
        let res = decode_image(&png_bytes, 500);
        match res {
            Err(CoreError::PixelBudgetExceeded {
                width,
                height,
                limit,
            }) => {
                assert_eq!(width, 100);
                assert_eq!(height, 100);
                assert_eq!(limit, 500);
            }
            other => panic!("expected PixelBudgetExceeded, got {other:?}"),
        }
    }

    #[test]
    fn test_truncated_jpeg_fails_gracefully() {
        let truncated = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46];
        let res = decode_image(&truncated, 40_000_000);
        assert!(res.is_err());
        match res {
            Err(CoreError::DecodeError(_)) => {}
            other => panic!("expected DecodeError, got {other:?}"),
        }
    }
}
