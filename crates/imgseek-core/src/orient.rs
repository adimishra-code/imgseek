use crate::format::ImageFormat;
use image::{imageops, DynamicImage};

/// EXIF orientation tag values (1..=8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifOrientation {
    Normal = 1,
    FlipHorizontal = 2,
    Rotate180 = 3,
    FlipVertical = 4,
    Transpose = 5,
    Rotate90 = 6,
    Transverse = 7,
    Rotate270 = 8,
}

impl ExifOrientation {
    pub fn from_u16(val: u16) -> Option<Self> {
        match val {
            1 => Some(Self::Normal),
            2 => Some(Self::FlipHorizontal),
            3 => Some(Self::Rotate180),
            4 => Some(Self::FlipVertical),
            5 => Some(Self::Transpose),
            6 => Some(Self::Rotate90),
            7 => Some(Self::Transverse),
            8 => Some(Self::Rotate270),
            _ => None,
        }
    }
}

/// Parse EXIF orientation from image raw bytes if JPEG or TIFF.
pub fn parse_exif_orientation(bytes: &[u8], format: ImageFormat) -> Option<ExifOrientation> {
    match format {
        ImageFormat::Jpeg => parse_jpeg_exif(bytes),
        ImageFormat::Tiff => parse_tiff_exif(bytes),
        _ => None,
    }
}

fn parse_jpeg_exif(bytes: &[u8]) -> Option<ExifOrientation> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }

    let mut idx = 2;
    while idx + 4 <= bytes.len() {
        if bytes[idx] != 0xFF {
            return None;
        }
        let marker = bytes[idx + 1];
        idx += 2;

        // Standalone markers without length
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }

        if idx + 2 > bytes.len() {
            return None;
        }
        let len = u16::from_be_bytes([bytes[idx], bytes[idx + 1]]) as usize;
        if len < 2 || idx + len > bytes.len() {
            return None;
        }

        // APP1 marker (0xE1) contains EXIF
        if marker == 0xE1 {
            let app1_payload = &bytes[idx + 2..idx + len];
            if app1_payload.len() >= 6 && app1_payload.starts_with(b"Exif\0\0") {
                let tiff_header = &app1_payload[6..];
                return parse_tiff_exif(tiff_header);
            }
        }

        idx += len;
    }
    None
}

fn parse_tiff_exif(bytes: &[u8]) -> Option<ExifOrientation> {
    if bytes.len() < 8 {
        return None;
    }

    let is_le = if bytes.starts_with(b"II*\0") {
        true
    } else if bytes.starts_with(b"MM\0*") {
        false
    } else {
        return None;
    };

    let read_u16 = |buf: &[u8]| -> Option<u16> {
        if buf.len() < 2 {
            return None;
        }
        if is_le {
            Some(u16::from_le_bytes([buf[0], buf[1]]))
        } else {
            Some(u16::from_be_bytes([buf[0], buf[1]]))
        }
    };

    let read_u32 = |buf: &[u8]| -> Option<u32> {
        if buf.len() < 4 {
            return None;
        }
        if is_le {
            Some(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]))
        } else {
            Some(u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]))
        }
    };

    let ifd0_offset = read_u32(&bytes[4..8])? as usize;
    if ifd0_offset + 2 > bytes.len() {
        return None;
    }

    let num_entries = read_u16(&bytes[ifd0_offset..ifd0_offset + 2])? as usize;
    let mut entry_offset = ifd0_offset + 2;

    for _ in 0..num_entries {
        if entry_offset + 12 > bytes.len() {
            return None;
        }
        let tag = read_u16(&bytes[entry_offset..entry_offset + 2])?;
        if tag == 0x0112 {
            // Orientation tag
            let val = read_u16(&bytes[entry_offset + 8..entry_offset + 10])?;
            return ExifOrientation::from_u16(val);
        }
        entry_offset += 12;
    }

    None
}

/// Applies EXIF orientation to a DynamicImage.
pub fn apply_orientation(img: DynamicImage, orientation: ExifOrientation) -> DynamicImage {
    match orientation {
        ExifOrientation::Normal => img,
        ExifOrientation::FlipHorizontal => img.fliph(),
        ExifOrientation::Rotate180 => img.rotate180(),
        ExifOrientation::FlipVertical => img.flipv(),
        ExifOrientation::Transpose => {
            // Transpose = flip horizontal + rotate 270 CW (or rotate90 + fliph)
            let flipped = img.fliph();
            imageops::rotate270(&flipped).into()
        }
        ExifOrientation::Rotate90 => img.rotate90(),
        ExifOrientation::Transverse => {
            // Transverse = flip horizontal + rotate 90 CW
            let flipped = img.fliph();
            imageops::rotate90(&flipped).into()
        }
        ExifOrientation::Rotate270 => img.rotate270(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn test_apply_orientation_rotate90() {
        // Create 2x1 image: pixel (0,0) is red, (1,0) is blue
        let mut img = RgbImage::new(2, 1);
        img.put_pixel(0, 0, Rgb([255, 0, 0]));
        img.put_pixel(1, 0, Rgb([0, 0, 255]));
        let dyn_img = DynamicImage::ImageRgb8(img);

        let rotated = apply_orientation(dyn_img, ExifOrientation::Rotate90);
        // After 90 deg clockwise rotation, dimensions should be 1x2
        assert_eq!(rotated.width(), 1);
        assert_eq!(rotated.height(), 2);
    }

    #[test]
    fn test_parse_tiff_orientation() {
        // Synthetic minimal TIFF with orientation = 6 (Rotate90)
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II*\0"); // LE magic
        tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset at 8
        tiff.extend_from_slice(&1u16.to_le_bytes()); // 1 entry
                                                     // Entry: tag 0x0112, type 3 (SHORT), count 1, value 6
        tiff.extend_from_slice(&0x0112u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&6u16.to_le_bytes());
        tiff.extend_from_slice(&[0, 0]); // pad to 4 bytes for value

        let ori = parse_exif_orientation(&tiff, ImageFormat::Tiff);
        assert_eq!(ori, Some(ExifOrientation::Rotate90));
    }
}
