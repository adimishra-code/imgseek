use image::{DynamicImage, Rgb, RgbImage};

/// Converts any DynamicImage to an 8-bit sRGB RGB image, compositing any alpha channel onto white.
pub fn composite_onto_white(img: DynamicImage) -> RgbImage {
    let width = img.width();
    let height = img.height();

    if let DynamicImage::ImageRgb8(rgb) = img {
        return rgb;
    }

    let rgba = img.into_rgba8();
    let mut out = RgbImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        if a == 255 {
            out.put_pixel(x, y, Rgb([r, g, b]));
        } else if a == 0 {
            out.put_pixel(x, y, Rgb([255, 255, 255]));
        } else {
            let a32 = a as u32;
            let inv_a = 255 - a32;
            let r_out = ((r as u32 * a32 + 255 * inv_a + 127) / 255) as u8;
            let g_out = ((g as u32 * a32 + 255 * inv_a + 127) / 255) as u8;
            let b_out = ((b as u32 * a32 + 255 * inv_a + 127) / 255) as u8;
            out.put_pixel(x, y, Rgb([r_out, g_out, b_out]));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    #[test]
    fn test_alpha_compositing_white_background() {
        let mut rgba: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(1, 1);
        rgba.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        let dyn_img = DynamicImage::ImageRgba8(rgba);

        let rgb = composite_onto_white(dyn_img);
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 255, 255]);
    }

    #[test]
    fn test_alpha_compositing_semi_transparent() {
        let mut rgba: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(1, 1);
        rgba.put_pixel(0, 0, Rgba([0, 0, 0, 128]));
        let dyn_img = DynamicImage::ImageRgba8(rgba);

        let rgb = composite_onto_white(dyn_img);
        assert_eq!(rgb.get_pixel(0, 0).0, [127, 127, 127]);
    }

    #[test]
    fn test_opaque_image_preserved() {
        let mut rgba: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(1, 1);
        rgba.put_pixel(0, 0, Rgba([12, 34, 56, 255]));
        let dyn_img = DynamicImage::ImageRgba8(rgba);

        let rgb = composite_onto_white(dyn_img);
        assert_eq!(rgb.get_pixel(0, 0).0, [12, 34, 56]);
    }

    #[test]
    fn test_16bit_image_converted_to_8bit() {
        let mut rgba16: image::ImageBuffer<image::Rgba<u16>, Vec<u16>> = ImageBuffer::new(1, 1);
        rgba16.put_pixel(0, 0, image::Rgba([65535, 0, 0, 65535]));
        let dyn_img = DynamicImage::ImageRgba16(rgba16);

        let rgb = composite_onto_white(dyn_img);
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 0, 0]);
    }
}
